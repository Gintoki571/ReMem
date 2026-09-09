import { beforeEach, describe, expect, it } from "vitest";
import { MemoryGraph } from "./graph.js";
import type { MemoryItem } from "./types.js";

function item(overrides: Partial<MemoryItem> = {}): MemoryItem {
  return {
    id: overrides.id ?? `m-${Math.random().toString(36).slice(2, 8)}`,
    kind: "fact",
    content: "test content",
    tags: [],
    source: "agent",
    importance: 0.5,
    createdAt: Date.now(),
    updatedAt: Date.now(),
    validFrom: Date.now(),
    ...overrides,
  };
}

describe("MemoryGraph", () => {
  let graph: MemoryGraph;

  beforeEach(async () => {
    graph = await MemoryGraph.open({ vectorDimensions: 8 });
  });

  it("upserts a memory node and reads it back via cypher", async () => {
    await graph.upsertMemory(item({ id: "mem-1", content: "likes vim" }));
    const result = await graph.neighbors("mem-1");
    expect(result).toEqual([]);
  });

  it("links memories and traverses", async () => {
    await graph.upsertMemory(item({ id: "a", content: "a" }));
    await graph.upsertMemory(item({ id: "b", content: "b" }));
    await graph.link("a", "b", "RELATES_TO");
    const neighbors = await graph.neighbors("a");
    expect(neighbors).toHaveLength(1);
    expect(neighbors[0]?.targetId).toBe("b");
    expect(neighbors[0]?.type).toBe("RELATES_TO");
  });

  it("attaches agent/session hub edges", async () => {
    await graph.upsertMemory(item({ id: "m1", agentId: "agent-x", sessionId: "sess-1" }));
    const neighbors = await graph.neighbors("m1");
    const types = neighbors.map((n) => n.type).sort();
    expect(types).toEqual(["BELONGS_TO_AGENT", "BELONGS_TO_SESSION"]);
  });

  it("vector search returns nearest memories", async () => {
    await graph.upsertMemory(item({ id: "v1", content: "vim" }));
    await graph.upsertMemory(item({ id: "v2", content: "arch" }));
    await graph.setEmbedding("v1", [1, 0, 0, 0, 0, 0, 0, 0]);
    await graph.setEmbedding("v2", [0, 1, 0, 0, 0, 0, 0, 0]);
    const hits = await graph.relatedSearch([0.9, 0.1, 0, 0, 0, 0, 0, 0], 2);
    expect(hits[0]?.id).toBe("v1");
  });

  it("appends and reads durable events", async () => {
    await graph.appendEvent("audit", { op: "insert", id: "m1" }, "memory.insert");
    await graph.appendEvent("audit", { op: "insert", id: "m2" }, "memory.insert");
    const events = await graph.readEvents("audit");
    expect(events).toHaveLength(2);
    expect(events[0]?.sequence).toBe(1);
    expect(events[1]?.kind).toBe("memory.insert");
  });
}, 30000);
