// src/application/services/InfrastructureSyncService.ts

import { getDatabase, schema } from '@infrastructure/database/index.js';
import { deleteVectorsByNode } from '@infrastructure/vector/VectorManager.js';
import type { GraphOperations } from '@application/operations/GraphOperations.js';
import type { Node, Edge } from '@core/index.js';
import { eq, and } from 'drizzle-orm';

/**
 * Service that synchronizes graph operations with secondary storage (SQLite, Vector Store)
 */
export class InfrastructureSyncService {
    constructor(private graphOperations: GraphOperations) {
        this.initializeListeners();
    }

    private initializeListeners(): void {
        const db = getDatabase();

        // Node synchronization
        this.graphOperations.on('afterAddNodes', ({ nodes }: { nodes: Node[] }) => {
            for (const node of nodes) {
                try {
                    db.insert(schema.nodes).values({
                        name: node.name,
                        nodeType: node.nodeType,
                        metadata: JSON.stringify(node.metadata),
                    }).onConflictDoNothing().run();
                } catch (error) {
                    console.error(`[Sync] Error syncing added node "${node.name}" to SQLite:`, error);
                }
            }
        });

        this.graphOperations.on('afterUpdateNodes', ({ nodes }: { nodes: Partial<Node>[] }) => {
            for (const node of nodes) {
                if (!node.name) continue;
                try {
                    db.update(schema.nodes)
                        .set({
                            ...(node.nodeType && { nodeType: node.nodeType }),
                            ...(node.metadata && { metadata: JSON.stringify(node.metadata) }),
                            updatedAt: new Date()
                        })
                        .where(eq(schema.nodes.name, node.name))
                        .run();
                } catch (error) {
                    // Node might not exist in SQLite if it was created before sync was active
                    // In that case, we should probably insert it, but for now just log
                    console.error(`[Sync] Error syncing updated node "${node.name}" to SQLite:`, error);
                }
            }
        });

        this.graphOperations.on('afterDeleteNodes', async ({ nodeNames }: { nodeNames: string[] }) => {
            for (const name of nodeNames) {
                try {
                    // Sync with SQLite
                    db.delete(schema.nodes).where(eq(schema.nodes.name, name)).run();
                    db.delete(schema.edges).where(eq(schema.edges.fromNode, name)).run();
                    db.delete(schema.edges).where(eq(schema.edges.toNode, name)).run();

                    // Sync with Vector Store
                    await deleteVectorsByNode(name);
                } catch (error) {
                    console.error(`[Sync] Error syncing deleted node "${name}":`, error);
                }
            }
        });

        // Edge synchronization
        this.graphOperations.on('afterAddEdges', ({ edges }: { edges: Edge[] }) => {
            for (const edge of edges) {
                try {
                    db.insert(schema.edges).values({
                        fromNode: edge.from,
                        toNode: edge.to,
                        edgeType: edge.edgeType,
                        weight: edge.weight ?? 1.0,
                    }).onConflictDoNothing().run();
                } catch (error) {
                    console.error(`[Sync] Error syncing added edge to SQLite:`, error);
                }
            }
        });

        this.graphOperations.on('afterDeleteEdges', ({ edges }: { edges: Edge[] }) => {
            for (const edge of edges) {
                try {
                    db.delete(schema.edges)
                        .where(
                            and(
                                eq(schema.edges.fromNode, edge.from),
                                eq(schema.edges.toNode, edge.to),
                                eq(schema.edges.edgeType, edge.edgeType)
                            )
                        )
                        .run();
                } catch (error) {
                    console.error(`[Sync] Error syncing deleted edge:`, error);
                }
            }
        });
    }
}
