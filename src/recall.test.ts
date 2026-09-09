import { beforeEach, describe, expect, it } from "vitest";
import { Recall } from "./recall.js";
import { RememStore } from "./store.js";
import type { MemoryItemInput } from "./types.js";

const FAKE_VECS: Record<string, number[]> = {
  deploy: [1, 0, 0],
  deploy2: [0.9, 0.1, 0],
  python: [0, 1, 0],
};

function input(overrides: Partial<MemoryItemInput> & { content: string }): MemoryItemInput {
  return {
    kind: "fact",
    tags: [],
    source: "agent",
    importance: 0.5,
    ...overrides,
  };
}

describe("Recall", () => {
  let store: RememStore;
  let recall: Recall;

  beforeEach(() => {
    store = new RememStore(":memory:");
    recall = new Recall(store);
    const a = store.insert(input({ content: "deploy script is at scripts/deploy.mjs", tags: ["deploy"] }));
    const b = store.insert(input({ content: "redeploy after schema migration", tags: ["deploy", "db"] }));
    const c = store.insert(input({ content: "python venv lives in .venv", tags: ["python"] }));
    for (const [id, key] of [
      [a.id, "deploy"],
      [b.id, "deploy2"],
      [c.id, "python"],
    ] as Array<[string, keyof typeof FAKE_VECS]>) {
      const vec = FAKE_VECS[key];
      if (vec) store.setEmbedding(id, vec);
    }
  });

  it("returns empty on empty store or no text", async () => {
    const empty = new Recall(new RememStore(":memory:"));
    expect(await empty.search({ text: "anything", k: 5 })).toEqual([]);
    expect(await recall.search({ text: "", k: 5 })).toEqual([]);
  });

  it("ranks by fts when no embedFn", async () => {
    const hits = await recall.search({ text: "deploy script", k: 5 });
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]?.item.content).toContain("deploy script");
    expect(hits[0]?.reasons.some((r) => r.startsWith("fts#"))).toBe(true);
  });

  it("fuses vector + fts rankings", async () => {
    const embedFn = async (text: string) => (text.includes("deploy") ? [1, 0, 0] : [0, 0, 0]);
    const hits = await recall.search({ text: "deploy", k: 5 }, embedFn);
    expect(hits[0]?.reasons).toContain("vector#1");
    expect(hits[0]?.reasons.some((r) => r.startsWith("fts#"))).toBe(true);
  });

  it("respects kinds filter", async () => {
    store.insert(input({ content: "decision to deploy at noon", kind: "decision" }));
    const hits = await recall.search({ text: "deploy", k: 10, kinds: ["decision"] });
    expect(hits).toHaveLength(1);
    expect(hits[0]?.item.kind).toBe("decision");
  });

  it("importance boosts score", async () => {
    store.insert(input({ content: "critical prod deploy rule", importance: 0.99 }));
    const hits = await recall.search({ text: "prod deploy rule", k: 10 });
    expect(hits[0]?.item.importance).toBe(0.99);
  });
});
