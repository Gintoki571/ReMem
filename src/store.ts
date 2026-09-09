import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { type MemoryItem, type MemoryItemInput, MemoryItemSchema, MemoryKindSchema } from "./types.js";

export type StoreFilter = {
  kinds?: string[];
  tags?: string[];
  agentId?: string;
  sessionId?: string;
  includeDeleted?: boolean;
  limit?: number;
};

type MemoryRow = {
  id: string;
  kind: string;
  content: string;
  tags: string;
  source: string;
  session_id: string | null;
  agent_id: string | null;
  importance: number;
  created_at: number;
  updated_at: number;
  valid_from: number;
  valid_to: number | null;
  embedding?: Buffer | null;
};

function rowToItem(row: MemoryRow): MemoryItem {
  const parsed = MemoryItemSchema.parse({
    id: row.id,
    kind: row.kind,
    content: row.content,
    tags: JSON.parse(row.tags) as string[],
    source: row.source,
    sessionId: row.session_id ?? undefined,
    agentId: row.agent_id ?? undefined,
    importance: row.importance,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    validFrom: row.valid_from,
    validTo: row.valid_to,
  });
  return parsed;
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS memories (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  tags TEXT NOT NULL DEFAULT '[]',
  source TEXT NOT NULL DEFAULT 'agent',
  session_id TEXT,
  agent_id TEXT,
  importance REAL NOT NULL DEFAULT 0.5,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  valid_from INTEGER NOT NULL,
  valid_to INTEGER,
  embedding BLOB
);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
CREATE INDEX IF NOT EXISTS idx_memories_agent ON memories(agent_id);
CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
  content, tags, content='memories', content_rowid='rowid'
);
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
  INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES ('delete', old.rowid, old.content, old.tags);
  INSERT INTO memories_fts(rowid, content, tags) VALUES (new.rowid, new.content, new.tags);
END;
`;

export class RememStore {
  private readonly db: DatabaseSync;
  private readonly stmts;

  constructor(dbPath: string) {
    if (dbPath !== ":memory:") {
      mkdirSync(dirname(dbPath), { recursive: true });
    }
    this.db = new DatabaseSync(dbPath);
    this.db.exec("PRAGMA journal_mode = WAL");
    this.db.exec("PRAGMA foreign_keys = ON");
    this.db.exec(SCHEMA);
    this.stmts = {
      insert: this.db.prepare(
        `INSERT INTO memories (id, kind, content, tags, source, session_id, agent_id, importance, created_at, updated_at, valid_from, valid_to)
         VALUES (@id, @kind, @content, @tags, @source, @session_id, @agent_id, @importance, @created_at, @updated_at, @valid_from, @valid_to)`,
      ),
      get: this.db.prepare(`SELECT * FROM memories WHERE id = ?`),
      update: this.db.prepare(
        `UPDATE memories SET kind = @kind, content = @content, tags = @tags, source = @source,
         session_id = @session_id, agent_id = @agent_id, importance = @importance, updated_at = @updated_at,
         valid_to = @valid_to WHERE id = @id`,
      ),
      softDelete: this.db.prepare(`UPDATE memories SET valid_to = ? WHERE id = ? AND valid_to IS NULL`),
      hardDelete: this.db.prepare(`DELETE FROM memories WHERE id = ?`),
      ftsSearch: this.db.prepare(
        `SELECT m.* FROM memories_fts f JOIN memories m ON m.rowid = f.rowid
         WHERE memories_fts MATCH ? LIMIT ?`,
      ),
      list: this.db.prepare(`SELECT * FROM memories ORDER BY importance DESC, updated_at DESC LIMIT ?`),
      count: this.db.prepare(`SELECT COUNT(*) AS n FROM memories`),
      setEmbedding: this.db.prepare(`UPDATE memories SET embedding = ? WHERE id = ?`),
      getEmbedding: this.db.prepare(`SELECT embedding FROM memories WHERE id = ? AND embedding IS NOT NULL`),
    };
  }

  insert(input: MemoryItemInput): MemoryItem {
    MemoryKindSchema.parse(input.kind);
    if (!input.content.trim()) throw new Error("content must be non-empty");
    const now = Date.now();
    const row: MemoryRow = {
      id: input.id ?? randomUUID(),
      kind: input.kind,
      content: input.content,
      tags: JSON.stringify(input.tags ?? []),
      source: input.source ?? "agent",
      session_id: input.sessionId ?? null,
      agent_id: input.agentId ?? null,
      importance: input.importance ?? 0.5,
      created_at: input.createdAt ?? now,
      updated_at: input.updatedAt ?? now,
      valid_from: input.validFrom ?? now,
      valid_to: null,
    };
    this.stmts.insert.run(row);
    return rowToItem(this.stmts.get.get(row.id) as MemoryRow);
  }

  get(id: string): MemoryItem | null {
    const row = this.stmts.get.get(id) as MemoryRow | undefined;
    return row ? rowToItem(row) : null;
  }

  update(id: string, patch: Partial<Omit<MemoryItemInput, "id">>): MemoryItem | null {
    const existing = this.get(id);
    if (!existing) return null;
    const merged = {
      id,
      kind: patch.kind ?? existing.kind,
      content: patch.content ?? existing.content,
      tags: JSON.stringify(patch.tags ?? existing.tags),
      source: patch.source ?? existing.source,
      session_id: "sessionId" in patch ? (patch.sessionId ?? null) : (existing.sessionId ?? null),
      agent_id: "agentId" in patch ? (patch.agentId ?? null) : (existing.agentId ?? null),
      importance: patch.importance ?? existing.importance,
      updated_at: Date.now(),
      valid_to: "validTo" in patch ? (patch.validTo ?? null) : (existing.validTo ?? null),
    };
    this.stmts.update.run(merged);
    return this.get(id);
  }

  softDelete(id: string): boolean {
    const res = this.stmts.softDelete.run(Date.now(), id);
    return res.changes > 0;
  }

  hardDelete(id: string): boolean {
    const res = this.stmts.hardDelete.run(id);
    return res.changes > 0;
  }

  ftsSearch(query: string, opts: { limit?: number; includeDeleted?: boolean } = {}): MemoryItem[] {
    const limit = opts.limit ?? 20;
    if (!query.trim()) return [];
    const rows = this.stmts.ftsSearch.all(query, limit * 4) as MemoryRow[];
    const items = rows.map(rowToItem).filter((i) => opts.includeDeleted || i.validTo == null);
    return items.slice(0, limit);
  }

  list(filter: StoreFilter = {}): MemoryItem[] {
    const limit = filter.limit ?? 100;
    const rows = this.stmts.list.all(limit * 4) as MemoryRow[];
    let items = rows.map(rowToItem);
    if (!filter.includeDeleted) items = items.filter((i) => i.validTo == null);
    if (filter.kinds) items = items.filter((i) => filter.kinds?.includes(i.kind));
    if (filter.tags) items = items.filter((i) => filter.tags?.some((t) => i.tags.includes(t)));
    if (filter.agentId) items = items.filter((i) => i.agentId === filter.agentId);
    if (filter.sessionId) items = items.filter((i) => i.sessionId === filter.sessionId);
    return items.slice(0, limit);
  }

  count(): number {
    const row = this.stmts.count.get() as { n: number };
    return row.n;
  }

  setEmbedding(id: string, vector: number[]): void {
    const buf = Buffer.from(new Float32Array(vector).buffer);
    this.stmts.setEmbedding.run(buf, id);
  }

  getEmbedding(id: string): number[] | null {
    const row = this.stmts.getEmbedding.get(id) as { embedding: Buffer } | undefined;
    if (!row) return null;
    return Array.from(
      new Float32Array(row.embedding.buffer, row.embedding.byteOffset, row.embedding.byteLength / 4),
    );
  }

  close(): void {
    this.db.close();
  }
}
