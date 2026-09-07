import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { DatabaseSync } from "node:sqlite";
import { describe, expect, it } from "vitest";
import { RememStore } from "./store.js";
import type { MemoryItemInput } from "./types.js";

function makeInput(overrides: Partial<MemoryItemInput> = {}): MemoryItemInput {
  return {
    kind: "fact",
    content: "the deploy script lives in scripts/deploy.mjs",
    tags: ["deploy"],
    source: "agent",
    importance: 0.5,
    ...overrides,
  };
}

describe("RememStore", () => {
  it("roundtrips insert/get", () => {
    const store = new RememStore(":memory:");
    const item = store.insert(makeInput());
    const got = store.get(item.id);
    expect(got).not.toBeNull();
    expect(got?.content).toBe(item.content);
    expect(got?.kind).toBe("fact");
    expect(got?.tags).toEqual(["deploy"]);
    expect(got?.validTo ?? null).toBeNull();
    store.close();
  });

  it("generates unique ids", () => {
    const store = new RememStore(":memory:");
    const a = store.insert(makeInput());
    const b = store.insert(makeInput());
    expect(a.id).not.toBe(b.id);
    store.close();
  });

  it("updates fields and bumps updatedAt", () => {
    const store = new RememStore(":memory:");
    const item = store.insert(makeInput({ importance: 0.2 }));
    const before = item.updatedAt;
    const updated = store.update(item.id, { content: "new content", importance: 0.9 });
    expect(updated?.content).toBe("new content");
    expect(updated?.importance).toBe(0.9);
    expect(updated?.updatedAt).toBeGreaterThanOrEqual(before);
    expect(updated?.createdAt).toBe(item.createdAt);
    store.close();
  });

  it("soft delete hides from list and fts but keeps row", () => {
    const store = new RememStore(":memory:");
    const item = store.insert(makeInput({ content: "secret sauce recipe" }));
    expect(store.softDelete(item.id)).toBe(true);
    expect(store.get(item.id)?.validTo).not.toBeNull();
    expect(store.list()).toHaveLength(0);
    expect(store.ftsSearch("secret sauce")).toHaveLength(0);
    expect(store.ftsSearch("secret sauce", { includeDeleted: true })).toHaveLength(1);
    store.close();
  });

  it("fts search matches content and tags", () => {
    const store = new RememStore(":memory:");
    store.insert(makeInput({ content: "postgres runs on port 5432" }));
    store.insert(makeInput({ content: "redis cache ttl is 300s", tags: ["cache", "redis"] }));
    const byContent = store.ftsSearch("postgres port");
    expect(byContent).toHaveLength(1);
    expect(byContent[0]?.content).toContain("postgres");
    const byTag = store.ftsSearch("redis");
    expect(byTag).toHaveLength(1);
    expect(byTag[0]?.tags).toContain("redis");
    store.close();
  });

  it("list filters by kind, tag, agent, session", () => {
    const store = new RememStore(":memory:");
    store.insert(makeInput({ kind: "mistake", tags: ["git"], agentId: "a1", sessionId: "s1" }));
    store.insert(makeInput({ kind: "decision", tags: ["infra"], agentId: "a2" }));
    store.insert(makeInput({ kind: "note", tags: ["git"], sessionId: "s2" }));
    expect(store.list({ kinds: ["mistake"] })).toHaveLength(1);
    expect(store.list({ tags: ["git"] })).toHaveLength(2);
    expect(store.list({ agentId: "a1" })).toHaveLength(1);
    expect(store.list({ sessionId: "s2" })).toHaveLength(1);
    store.close();
  });

  it("uses WAL journal mode on file db", () => {
    const dir = mkdtempSync(join(tmpdir(), "remem-test-"));
    try {
      const store = new RememStore(join(dir, "mem.db"));
      const mode = (store as unknown as { db: DatabaseSync }).db
        .prepare("PRAGMA journal_mode")
        .get()?.journal_mode;
      expect(mode).toBe("wal");
      store.close();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("persists across reopen (file db)", () => {
    const dir = mkdtempSync(join(tmpdir(), "remem-test-"));
    try {
      const path = join(dir, "mem.db");
      const s1 = new RememStore(path);
      const item = s1.insert(makeInput({ content: "durable fact" }));
      s1.close();
      const s2 = new RememStore(path);
      expect(s2.get(item.id)?.content).toBe("durable fact");
      s2.close();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("embedding blob roundtrip", () => {
    const store = new RememStore(":memory:");
    const item = store.insert(makeInput());
    const vec = [0.1, -0.2, 0.3, 0.4];
    store.setEmbedding(item.id, vec);
    const got = store.getEmbedding(item.id);
    expect(got?.length).toBe(4);
    expect(got?.[0]).toBeCloseTo(0.1, 5);
    expect(got?.[1]).toBeCloseTo(-0.2, 5);
    store.close();
  });

  it("rejects invalid kind via schema", () => {
    const store = new RememStore(":memory:");
    expect(() => store.insert(makeInput({ kind: "bogus" as "fact" }))).toThrow();
    expect(() => store.insert(makeInput({ content: "" }))).toThrow();
    store.close();
  });
});
