// src/infrastructure/storage/SqliteStorage.ts

import { getDatabase, schema } from '@infrastructure/database/index.js';
import type { IStorage } from './IStorage.js';
import type { Edge, Graph, Node } from '@core/index.js';
import { eq, and } from 'drizzle-orm';

/**
 * SQLite-based storage implementation.
 * Reads and writes graph data directly from/to SQLite, eliminating O(N) file load.
 */
export class SqliteStorage implements IStorage {
    /**
     * Loads the entire knowledge graph from SQLite.
     * This is still O(N) but much faster than file I/O and can leverage indices.
     */
    async loadGraph(): Promise<Graph> {
        const db = getDatabase();

        // Load nodes
        const nodeRows = db.select().from(schema.nodes).all();
        const nodes: Node[] = nodeRows.map(row => ({
            type: 'node' as const,
            name: row.name,
            nodeType: row.nodeType,
            metadata: row.metadata ? JSON.parse(row.metadata) : [],
            version: row.version,
        }));

        // Load edges
        const edgeRows = db.select().from(schema.edges).all();
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

        // This is the "legacy" path - used by NodeManager.addNodes etc.
        // We still support it for backward compatibility.
        // In the new architecture, InfrastructureSyncService handles individual writes.

        // For nodes: use upsert
        for (const node of graph.nodes) {
            db.insert(schema.nodes)
                .values({
                    name: node.name,
                    nodeType: node.nodeType,
                    metadata: JSON.stringify(node.metadata || []),
                })
                .onConflictDoUpdate({
                    target: schema.nodes.name,
                    set: {
                        nodeType: node.nodeType,
                        metadata: JSON.stringify(node.metadata || []),
                        updatedAt: new Date(),
                    },
                })
                .run();
        }

        // For edges: use upsert based on (from, to, type) composite key
        // Note: SQLite doesn't have a simple composite unique constraint upsert,
        // so we delete-then-insert for simplicity
        for (const edge of graph.edges) {
            // Check if edge exists
            const existing = db.select()
                .from(schema.edges)
                .where(and(
                    eq(schema.edges.fromNode, edge.from),
                    eq(schema.edges.toNode, edge.to),
                    eq(schema.edges.edgeType, edge.edgeType)
                ))
                .get();

            if (!existing) {
                db.insert(schema.edges)
                    .values({
                        fromNode: edge.from,
                        toNode: edge.to,
                        edgeType: edge.edgeType,
                        weight: edge.weight ?? 1.0,
                    })
                    .run();
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
