import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Remem } from "./remem.js";

describe("Remem (system)", () => {
  let dir: string;
  let remem: Remem;

  beforeEach(async () => {
    dir = mkdtempSync(join(tmpdir(), "remem-sys-"));
    remem = await Remem.open({ dataDir: dir, device: "cpu" });
  });

  afterEach(async () => {
    await remem.close();
    rmSync(dir, { recursive: true, force: true });
  });

  it("remembers a memory and recalls it by meaning", async () => {
    await remem.remember({
      kind: "preference",
      content: "the user prefers vim keybindings",
      tags: ["editor"],
    });
    await remem.remember({ kind: "fact", content: "the capital of france is paris" });
    const hits = await remem.recall({ text: "what editor does bindesh like", k: 2 });
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]?.item.content).toContain("vim");
  }, 30000);

  it("records durable events for every memory", async () => {
    await remem.remember({ kind: "note", content: "event one" });
    await remem.remember({ kind: "note", content: "event two" });
    const events = await remem.events();
    expect(events.length).toBeGreaterThanOrEqual(2);
    expect(events[0]?.kind).toBe("memory.insert");
  }, 30000);

  it("survives reopen with persisted sqlite and graph", async () => {
    const item = await remem.remember({ kind: "fact", content: "durable across reopen" });
    await remem.close();
    const remem2 = await Remem.open({ dataDir: dir, device: "cpu" });
    try {
      expect(remem2.store.get(item.id)?.content).toBe("durable across reopen");
      const hits = await remem2.recall({ text: "durable across reopen", k: 1 });
      expect(hits[0]?.item.id).toBe(item.id);
    } finally {
      await remem2.close();
    }
  }, 60000);

  it("links two memories", async () => {
    const a = await remem.remember({ kind: "decision", content: "decision a" });
    const b = await remem.remember({ kind: "mistake", content: "mistake b" });
    await remem.link(a.id, b.id, "CAUSED_BY");
    const neighbors = await remem.graph.neighbors(a.id);
    expect(neighbors.some((n) => n.targetId === b.id && n.type === "CAUSED_BY")).toBe(true);
  }, 30000);
}, 120000);
