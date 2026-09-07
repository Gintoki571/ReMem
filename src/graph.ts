import { Database, type Transaction } from "@hajewski/latticedb";
import type { MemoryItem } from "./types.js";

export type GraphOptions = {
  dbPath?: string;
  vectorDimensions?: number;
};

export type LinkRow = { targetId: string; type: string };

// ponytail: memory id string is stored as node property; bigint rowids stay internal to latticedb
export class MemoryGraph {
  private readonly db: Database;

  private constructor(db: Database) {
    this.db = db;
  }

  static async open(options: GraphOptions = {}): Promise<MemoryGraph> {
    const db = new Database(options.dbPath ?? ":memory:", {
      create: true,
      enableVectors: true,
      vectorDimensions: options.vectorDimensions ?? 768,
    });
    await db.open();
    return new MemoryGraph(db);
  }

  private async findMemoryNodeId(txn: Transaction, memoryId: string): Promise<bigint | null> {
    const result = await txn.query("MATCH (a:Memory {id: $id}) RETURN a", { id: memoryId });
    const v = result.rows[0]?.a;
    return typeof v === "bigint" ? v : null;
  }

  async upsertMemory(item: MemoryItem): Promise<void> {
    await this.db.write(async (txn) => {
      const nodeId = await this.findMemoryNodeId(txn, item.id);
      if (nodeId === null) {
        await txn.createNode({
          labels: ["Memory", item.kind],
          properties: {
            id: item.id,
            content: item.content,
            tags: item.tags.join(","),
            source: item.source,
            importance: item.importance,
          },
        });
      } else {
        await txn.setProperty(nodeId, "content", item.content);
        await txn.setProperty(nodeId, "tags", item.tags.join(","));
        await txn.setProperty(nodeId, "importance", item.importance);
      }
      const memNode = await this.findMemoryNodeId(txn, item.id);
      if (memNode !== null) {
        if (item.agentId) await this.ensureEdge(txn, memNode, `agent:${item.agentId}`, "BELONGS_TO_AGENT");
        if (item.sessionId)
          await this.ensureEdge(txn, memNode, `session:${item.sessionId}`, "BELONGS_TO_SESSION");
      }
    });
  }

  async setEmbedding(memoryId: string, vector: number[]): Promise<void> {
    await this.db.write(async (txn) => {
      const nodeId = await this.findMemoryNodeId(txn, memoryId);
      if (nodeId !== null) {
        await txn.setVector(nodeId, "embedding", new Float32Array(vector));
      }
    });
  }

  async link(fromMemoryId: string, toMemoryId: string, type: string): Promise<void> {
    await this.db.write(async (txn) => {
      const from = await this.findMemoryNodeId(txn, fromMemoryId);
      const to = await this.findMemoryNodeId(txn, toMemoryId);
      if (from !== null && to !== null) {
        await txn.createEdge(from, to, type);
      }
    });
  }

  async neighbors(memoryId: string): Promise<LinkRow[]> {
    const result = await this.db.query(
      "MATCH (a:Memory {id: $id})-[r]->(b) RETURN b.id AS targetId, type(r) AS type",
      { id: memoryId },
    );
    return result.rows.map((row) => ({
      targetId: String(row.targetId),
      type: String(row.type),
    }));
  }

  async relatedSearch(vector: number[], k = 5): Promise<Array<{ id: string; distance: number }>> {
    const results = await this.db.vectorSearch(new Float32Array(vector), { k });
    const mapped: Array<{ id: string; distance: number }> = [];
    for (const r of results) {
      const row = await this.db.query("MATCH (m:Memory) WHERE id(m) = $nid RETURN m.id AS mid", {
        nid: r.nodeId,
      });
      const mid = row.rows[0]?.mid;
      if (typeof mid === "string") mapped.push({ id: mid, distance: r.distance });
    }
    return mapped;
  }

  async appendEvent(
    stream: string,
    payload: Record<string, string | number | boolean>,
    kind: string,
  ): Promise<void> {
    await this.db.write(async (txn) => {
      txn.publishStream(stream, payload, kind);
    });
  }

  async readEvents(
    stream: string,
    afterSequence = 0,
    limit = 100,
  ): Promise<Array<{ sequence: number; kind: string; payload: unknown }>> {
    const records = await this.db.readStream(stream, { afterSequence: BigInt(afterSequence), limit });
    return records.map((r) => ({ sequence: Number(r.sequence), kind: r.kind, payload: r.payload }));
  }

  async close(): Promise<void> {
    await this.db.close();
  }

  private async ensureEdge(
    txn: Transaction,
    fromId: bigint,
    hubNodeId: string,
    edgeType: string,
  ): Promise<void> {
    const hubResult = await txn.query("MATCH (h:Hub {id: $id}) RETURN h", { id: hubNodeId });
    const hubVal = hubResult.rows[0]?.h;
    let hubId: bigint;
    if (typeof hubVal === "bigint") {
      hubId = hubVal;
    } else {
      const hub = await txn.createNode({ labels: ["Hub"], properties: { id: hubNodeId } });
      hubId = hub.id;
    }
    await txn.createEdge(fromId, hubId, edgeType);
  }
}
