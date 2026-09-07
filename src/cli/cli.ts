#!/usr/bin/env node
import { join } from "node:path";
import { Remem } from "../remem.js";

const DATA_DIR = process.env.REMEM_DATA_DIR ?? join(process.env.HOME ?? ".", ".remem");

type Args = {
  command: string;
  positional: string[];
  flags: Record<string, string>;
};

function parseArgs(argv: string[]): Args {
  const flags: Record<string, string> = {};
  const positional: string[] = [];
  let command = "";
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i] ?? "";
    if (a.startsWith("--")) {
      flags[a.slice(2)] = argv[i + 1] ?? "";
      i++;
    } else if (!command) {
      command = a;
    } else {
      positional.push(a);
    }
  }
  return { command, positional, flags };
}

function usage(): string {
  return [
    "remem - long-term memory for agents",
    "",
    "  remem remember <kind> <text...> [--tags a,b] [--agent id] [--session id] [--importance 0.8]",
    "  remem recall <query...> [--k 5] [--kind fact,decision] [--tags a,b]",
    "  remem list [--k 20] [--kind fact]",
    "  remem link <fromId> <toId> [--type RELATES_TO]",
    "  remem events [--after 0]",
    "  remem forget <id>",
    "  remem stats",
  ].join("\n");
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  if (!args.command || args.command === "help") {
    console.log(usage());
    process.exit(0);
  }
  const remem = await Remem.open({ dataDir: join(DATA_DIR, "data") });
  try {
    switch (args.command) {
      case "remember": {
        const kind = args.positional[0];
        const text = args.positional.slice(1).join(" ");
        if (!kind || !text) {
          console.error("usage: remem remember <kind> <text...>");
          process.exit(1);
        }
        const item = await remem.remember({
          kind: kind as "fact",
          content: text,
          tags: args.flags.tags ? args.flags.tags.split(",") : [],
          ...(args.flags.agent ? { agentId: args.flags.agent } : {}),
          ...(args.flags.session ? { sessionId: args.flags.session } : {}),
          ...(args.flags.importance ? { importance: Number(args.flags.importance) } : {}),
        });
        console.log(JSON.stringify({ id: item.id, kind: item.kind, content: item.content }));
        break;
      }
      case "recall": {
        const hits = await remem.recall({
          text: args.positional.join(" "),
          k: args.flags.k ? Number(args.flags.k) : 5,
          kinds: args.flags.kind ? (args.flags.kind.split(",") as Array<"fact">) : undefined,
          tags: args.flags.tags ? args.flags.tags.split(",") : undefined,
        });
        for (const hit of hits) {
          console.log(
            `${hit.item.id}  ${hit.item.kind}  ${hit.score.toFixed(4)}  [${hit.reasons.join(",")}]  ${hit.item.content}`,
          );
        }
        break;
      }
      case "list": {
        for (const item of remem.store.list({ limit: args.flags.k ? Number(args.flags.k) : 20 })) {
          console.log(`${item.id}  ${item.kind}  ${item.content}`);
        }
        break;
      }
      case "link": {
        const [from, to] = args.positional;
        if (!from || !to) {
          console.error("usage: remem link <fromId> <toId>");
          process.exit(1);
        }
        await remem.link(from, to, args.flags.type ?? "RELATES_TO");
        console.log("linked");
        break;
      }
      case "events": {
        for (const e of await remem.events(args.flags.after ? Number(args.flags.after) : 0)) {
          console.log(`${e.sequence}  ${e.kind}  ${JSON.stringify(e.payload)}`);
        }
        break;
      }
      case "forget": {
        const id = args.positional[0];
        if (!id) {
          console.error("usage: remem forget <id>");
          process.exit(1);
        }
        console.log(remem.store.softDelete(id) ? "forgotten" : "not found");
        break;
      }
      case "stats": {
        console.log(
          JSON.stringify({
            memories: remem.store.count(),
            device: remem.embeddingDevice,
            dims: remem.embeddingDims,
          }),
        );
        break;
      }
      default:
        console.error(`unknown command: ${args.command}`);
        console.log(usage());
        process.exit(1);
    }
  } finally {
    await remem.close();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
