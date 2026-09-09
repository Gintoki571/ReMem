import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";
import { MODEL_AVAILABLE } from "../model-availability.js";

const tsx = "npx";
const cli = join(import.meta.dirname, "cli.ts");
let dataDir: string;

function runCli(args: string[]): string {
  return execFileSync(tsx, ["tsx", cli, ...args], {
    env: { ...process.env, REMEM_DATA_DIR: dataDir },
    encoding: "utf8",
  });
}

describe.skipIf(!MODEL_AVAILABLE)(
  "CLI",
  () => {
    beforeAll(() => {
      dataDir = mkdtempSync(join(tmpdir(), "remem-cli-"));
    });

    it("remembers then recalls", () => {
      runCli(["remember", "preference", "test user likes terse answers", "--tags", "style"]);
      const out = runCli(["recall", "terse answers style"]);
      expect(out).toContain("test user likes terse answers");
    }, 60000);

    it("prints stats as json", () => {
      const out = runCli(["stats"]);
      const stats = JSON.parse(out.trim()) as { memories: number; dims: number };
      expect(stats.memories).toBeGreaterThan(0);
      expect(stats.dims).toBe(768);
    }, 60000);
  },
  120000,
);
