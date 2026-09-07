import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Embedder } from "./embedder.js";
import { MODEL_AVAILABLE } from "./model-availability.js";

let embedder: Embedder;

beforeEach(async () => {
  embedder = await Embedder.create({ device: "cpu" });
});

afterEach(() => {
  // transformers.js models close with the process; no explicit dispose API in v4
});

describe.skipIf(!MODEL_AVAILABLE)("Embedder", () => {
  it("returns 768-dim normalized vectors", async () => {
    const vec = await embedder.embed("hello world");
    expect(vec).toHaveLength(768);
    const norm = Math.sqrt(vec.reduce((s, x) => s + x * x, 0));
    expect(norm).toBeCloseTo(1.0, 2);
  });

  it("scores related text higher than unrelated", async () => {
    const batch = await embedder.embedBatch([
      "my favorite editor is vim",
      "i use vim as my editor",
      "the capital of france is paris",
    ]);
    const [a, b, c] = batch;
    if (!a || !b || !c) throw new Error("batch returned fewer vectors");
    const dot = (x: number[], y: number[]) => {
      let s = 0;
      const n = Math.min(x.length, y.length);
      for (let i = 0; i < n; i++) s += (x[i] ?? 0) * (y[i] ?? 0);
      return s;
    };
    expect(dot(a, b)).toBeGreaterThan(0.7);
    expect(dot(a, c)).toBeLessThan(0.7);
  }, 30000);

  it("reports device and dims", () => {
    expect(embedder.dims).toBe(768);
    expect(["cpu", "webgpu"]).toContain(embedder.device);
  });
}, 60000);
