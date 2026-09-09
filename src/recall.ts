import type { RememStore, StoreFilter } from "./store.js";
import type { MemoryItem, RecallHit, RecallQuery } from "./types.js";
import { RecallQuerySchema } from "./types.js";

export type RecallOptions = {
  vectorWeight?: number;
  ftsWeight?: number;
  recencyHalfLifeDays?: number;
  rrfK?: number;
};

function cosine(a: number[], b: number[]): number {
  let dot = 0;
  let na = 0;
  let nb = 0;
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    dot += x * y;
    na += x * x;
    nb += y * y;
  }
  if (na === 0 || nb === 0) return 0;
  return dot / Math.sqrt(na * nb);
}

function recencyScore(updatedAt: number, halfLifeDays: number, now: number): number {
  const ageDays = Math.max(0, (now - updatedAt) / 86_400_000);
  return 0.5 ** (ageDays / halfLifeDays);
}

function rrf(rank: number, k: number): number {
  return 1 / (k + rank + 1);
}

// ponytail: brute-force cosine scan over all stored embeddings; sqlite-side vector index if stores exceed ~50k
export class Recall {
  constructor(
    private readonly store: RememStore,
    private readonly options: RecallOptions = {},
  ) {}

  async search(
    query: Omit<RecallQuery, "includeDeleted"> & { includeDeleted?: boolean; embedding?: number[] },
    embedFn?: (text: string) => Promise<number[]>,
  ): Promise<RecallHit[]> {
    const q = RecallQuerySchema.parse(query);
    const now = Date.now();
    const vectorWeight = this.options.vectorWeight ?? 1.0;
    const ftsWeight = this.options.ftsWeight ?? 1.0;
    const halfLife = this.options.recencyHalfLifeDays ?? 30;
    const k = this.options.rrfK ?? 60;

    const candidates = new Map<string, MemoryItem>();
    const filter: StoreFilter = { includeDeleted: q.includeDeleted, limit: 10_000 };
    if (q.kinds) filter.kinds = [...q.kinds];
    if (q.tags) filter.tags = [...q.tags];
    if (q.agentId) filter.agentId = q.agentId;
    if (q.sessionId) filter.sessionId = q.sessionId;
    for (const item of this.store.list(filter)) {
      candidates.set(item.id, item);
    }
    if (candidates.size === 0) return [];

    const vectorRanked: string[] = [];
    if (q.text.trim() && embedFn) {
      const qvec = query.embedding ?? (await embedFn(q.text));
      const scored: Array<{ id: string; score: number }> = [];
      for (const item of candidates.values()) {
        const vec = this.store.getEmbedding(item.id);
        if (vec) scored.push({ id: item.id, score: cosine(qvec, vec) });
      }
      scored.sort((a, b) => b.score - a.score);
      vectorRanked.push(...scored.slice(0, q.k).map((s) => s.id));
    }

    const ftsRanked: string[] = [];
    if (q.text.trim()) {
      for (const item of this.store.ftsSearch(q.text, { limit: q.k, includeDeleted: q.includeDeleted })) {
        if (candidates.has(item.id)) ftsRanked.push(item.id);
      }
    }

    const scores = new Map<string, { score: number; reasons: string[] }>();
    vectorRanked.forEach((id, rank) => {
      const entry = scores.get(id) ?? { score: 0, reasons: [] };
      entry.score += vectorWeight * rrf(rank, k);
      entry.reasons.push(`vector#${rank + 1}`);
      scores.set(id, entry);
    });
    ftsRanked.forEach((id, rank) => {
      const entry = scores.get(id) ?? { score: 0, reasons: [] };
      entry.score += ftsWeight * rrf(rank, k);
      entry.reasons.push(`fts#${rank + 1}`);
      scores.set(id, entry);
    });

    const hits: RecallHit[] = [];
    for (const [id, { score, reasons }] of scores) {
      const item = candidates.get(id);
      if (!item) continue;
      const recency = recencyScore(item.updatedAt, halfLife, now);
      const total = score * (0.5 + item.importance) * (0.7 + 0.3 * recency);
      const finalReasons = [...reasons];
      if (recency > 0.9) finalReasons.push("recent");
      if (item.importance >= 0.8) finalReasons.push("important");
      hits.push({ item, score: total, reasons: finalReasons });
    }
    hits.sort((a, b) => b.score - a.score);
    return hits.slice(0, q.k);
  }
}
