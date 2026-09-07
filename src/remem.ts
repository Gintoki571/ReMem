import { join } from "node:path";
import { Embedder } from "./embedder.js";
import { MemoryGraph } from "./graph.js";
import { Recall, type RecallOptions } from "./recall.js";
import { RememStore } from "./store.js";
import type { MemoryItem, MemoryItemInput, RecallHit, RecallQuery } from "./types.js";

export type RememOptions = {
  dataDir?: string;
  graphDbPath?: string;
  device?: "webgpu" | "cpu" | "auto";
  recall?: RecallOptions;
};

// ponytail: dataDir holds both sqlite + latticedb files; embedder always loaded - lazy-load if startup cost matters
export class Remem {
  readonly store: RememStore;
  readonly graph: MemoryGraph;
  private readonly embedder: Embedder;
  private readonly recallEngine: Recall;
  private closed = false;

  private constructor(
    store: RememStore,
    graph: MemoryGraph,
    embedder: Embedder,
    recallOptions: RecallOptions,
  ) {
    this.store = store;
    this.graph = graph;
    this.embedder = embedder;
    this.recallEngine = new Recall(store, recallOptions);
  }

  static async open(options: RememOptions = {}): Promise<Remem> {
    const dataDir = options.dataDir ?? join(process.cwd(), ".remem-data");
    const store = new RememStore(join(dataDir, "memories.db"));
    const graph = await MemoryGraph.open({ dbPath: options.graphDbPath ?? join(dataDir, "graph.lattice") });
    const embedder = await Embedder.create({ device: options.device ?? "auto" });
    return new Remem(store, graph, embedder, options.recall ?? {});
  }

  async remember(
    input: Pick<MemoryItemInput, "kind" | "content"> & Partial<Omit<MemoryItemInput, "kind" | "content">>,
  ): Promise<MemoryItem> {
    const item = this.store.insert({
      ...input,
      tags: input.tags ?? [],
      source: input.source ?? "agent",
      importance: input.importance ?? 0.5,
    });
    const vector = await this.embedder.embed(item.content);
    this.store.setEmbedding(item.id, vector);
    await this.graph.upsertMemory(item);
    await this.graph.setEmbedding(item.id, vector);
    await this.graph.appendEvent(
      "remem-events",
      {
        op: "insert",
        id: item.id,
        kind: item.kind,
        agent: item.agentId ?? "",
        session: item.sessionId ?? "",
      },
      "memory.insert",
    );
    return { ...item, embedding: undefined } as MemoryItem;
  }

  async recall(
    query: Omit<RecallQuery, "includeDeleted"> & { includeDeleted?: boolean },
  ): Promise<RecallHit[]> {
    return this.recallEngine.search(query, (text) => this.embedder.embed(text));
  }

  async link(fromId: string, toId: string, type = "RELATES_TO"): Promise<void> {
    await this.graph.link(fromId, toId, type);
  }

  async events(
    afterSequence = 0,
    limit = 100,
  ): Promise<Array<{ sequence: number; kind: string; payload: unknown }>> {
    return this.graph.readEvents("remem-events", afterSequence, limit);
  }

  get embeddingDevice(): string {
    return this.embedder.device;
  }

  get embeddingDims(): number {
    return this.embedder.dims;
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.store.close();
    await this.graph.close();
  }
}
