// src/infrastructure/storage/SqliteStorage.ts

import { getDatabase, schema } from '@infrastructure/database/index.js';
import type { IStorage } from './IStorage.js';
import type { Edge, Graph, Node } from '@core/index.js';
import { eq, and, inArray, sql } from 'drizzle-orm';
import { ConcurrencyError } from '@core/errors/index.js';

/**
 * SQLite-based storage implementation.
 * Reads and writes graph data directly from/to SQLite, eliminating O(N) file load.
 */
export class SqliteStorage implements IStorage {
    /**
     * Loads the entire knowledge graph from SQLite.
     * This is still O(N) but much faster than file I/O and can leverage indices.
     */
    async loadGraph(limit?: number, offset?: number): Promise<Graph> {
        const db = getDatabase();

        // Load nodes with pagination
        // Using dynamic query construction with Drizzle
        let query = db.select().from(schema.nodes).$dynamic();

        if (limit) {
            query = query.limit(limit);
        }
        if (offset) {
            query = query.offset(offset);
        }

        const nodeRows = query.all();

        const nodes: Node[] = nodeRows.map(row => ({
            type: 'node' as const,
            name: row.name,
            nodeType: row.nodeType,
            metadata: (row.metadata as Record<string, unknown>) || {},
            version: row.version,
        }));

        // Load edges
        // Ideally we should filter edges to only include those relevant to the loaded nodes
        // if we are paginating, otherwise we return partial graph.
        // For partial graph reads, we typically assume we want edges between loaded nodes.
        let edgeRows: any[] = [];

        if (limit) {
            const loadedNodeNames = nodes.map(n => n.name);
            if (loadedNodeNames.length > 0) {
                edgeRows = db.select()
                    .from(schema.edges)
                    .where(and(
                        inArray(schema.edges.fromNode, loadedNodeNames),
                        inArray(schema.edges.toNode, loadedNodeNames)
                    ))
                    .all();
            } else {
                edgeRows = [];
            }
        } else {
            edgeRows = db.select().from(schema.edges).all();
        }

        const edges: Edge[] = edgeRows.map(row => ({
            type: 'edge' as const,
            from: row.fromNode,
            to: row.toNode,
            edgeType: row.edgeType,
            weight: row.weight ?? 1.0,
        }));

        return { nodes, edges };
    }

    /**
     * Saves the entire knowledge graph to SQLite.
     * Uses upsert pattern to handle both new and existing entities.
     * NOTE: For large graphs, prefer individual add/update/delete operations.
     */
    async saveGraph(graph: Graph): Promise<void> {
        const db = getDatabase();

        // 1. Batch Insert/Upsert Nodes
        if (graph.nodes.length > 0) {
            const nodeValues = graph.nodes.map(node => ({
                name: node.name,
                nodeType: node.nodeType,
                metadata: node.metadata || {},
            }));

            // Use sql for accessing excluded values in upsert
            const { sql } = await import('drizzle-orm');

            db.insert(schema.nodes)
                .values(nodeValues)
                .onConflictDoUpdate({
                    target: schema.nodes.name,
                    set: {
                        nodeType: sql`excluded.node_type`,
                        metadata: sql`excluded.metadata`,
                        updatedAt: new Date(),
                    },
                })
                .run();
        }

        // 2. Batch Insert Edges
        // Uses ON CONFLICT DO NOTHING (requires unique constraint on from|to|type)
        if (graph.edges.length > 0) {
            const edgeValues = graph.edges.map(edge => ({
                fromNode: edge.from,
                toNode: edge.to,
                edgeType: edge.edgeType,
                weight: edge.weight ?? 1.0,
            }));

            db.insert(schema.edges)
                .values(edgeValues)
                .onConflictDoNothing()
                .run();
        }
    }




    /**
     * Loads specific nodes by their names.
     */
    async loadNodes(names: string[]): Promise<Node[]> {
        const db = getDatabase();
        if (names.length === 0) return [];

        const rows = db.select()
            .from(schema.nodes)
            .where(inArray(schema.nodes.name, names))
            .all();

        return rows.map(row => ({
            type: 'node' as const,
            name: row.name,
            nodeType: row.nodeType,
            metadata: (row.metadata as Record<string, unknown>) || {},
            version: row.version,
        }));
    }

    /**
     * Updates specific nodes with optimistic locking support.
     * @throws ConcurrencyError if version mismatch.
     */
    async updateNodes(nodes: Node[]): Promise<void> {
        const db = getDatabase();

        for (const node of nodes) {
            // Ensure version is present for optimistic locking
            if (node.version === undefined) {
                // If version is missing, we must fetch it first? 
                // Or we assume 1? Or we just fail?
                // For now, if version is missing, we might fallback to blind update OR throw.
                // Given this is for CRIT-2 (Security/Integrity), we SHOULD throw or at least fetch.
                // But fetch loses the point of "check what I saw is what is there".
                // If I didn't see a version, I can't optimistically lock.
                // Defaulting to "blind overwrite" (force) might be acceptable if explicitly intended, 
                // but here we want safety.
                throw new Error(`Optimistic locking failed: Node '${node.name}' has no version.`);
            }

            const result = db.update(schema.nodes)
                .set({
                    nodeType: node.nodeType,
                    metadata: node.metadata,
                    // Increment version atomicaly
                    version: sql`${schema.nodes.version} + 1`,
                    updatedAt: new Date(),
                })
                .where(and(
                    eq(schema.nodes.name, node.name),
                    eq(schema.nodes.version, node.version)
                ))
                .run();

            if (result.changes === 0) {
                // Check if node exists to distinguish "Not Found" vs "Locked"
                const exists = db.select({ count: sql<number>`count(*)` })
                    .from(schema.nodes)
                    .where(eq(schema.nodes.name, node.name))
                    .get();

                if (!exists || exists.count === 0) {
                    throw new Error(`Node not found: ${node.name}`);
                }

                // Node exists but update failed => Version mismatch
                throw new ConcurrencyError(node.name, node.version as number);
            }
        }
    }

    /**
     * Loads specific edges by their composite IDs (from|to|type).
     */
    async loadEdgesByIds(edgeIds: string[]): Promise<Edge[]> {

        const db = getDatabase();
        const result: Edge[] = [];

        for (const id of edgeIds) {
            const [from, to, edgeType] = id.split('|');
            if (!from || !to || !edgeType) continue;

            const row = db.select()
                .from(schema.edges)
                .where(and(
                    eq(schema.edges.fromNode, from),
                    eq(schema.edges.toNode, to),
                    eq(schema.edges.edgeType, edgeType)
                ))
                .get();

            if (row) {
                result.push({
                    type: 'edge' as const,
                    from: row.fromNode,
                    to: row.toNode,
                    edgeType: row.edgeType,
                    weight: row.weight ?? 1.0,
                });
            }
        }

        return result;
    }
}
