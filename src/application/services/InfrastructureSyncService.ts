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
    private cleanupFunctions: Array<() => void> = [];

    constructor(private graphOperations: GraphOperations) {
        this.initializeListeners();
    }

    /**
     * Cleanup event listeners to prevent memory leaks
     */
    public cleanup(): void {
        this.cleanupFunctions.forEach(fn => fn());
        this.cleanupFunctions = [];
    }

    private initializeListeners(): void {
        const db = getDatabase();

        // Node synchronization
        const afterAddNodesHandler = ({ nodes }: { nodes: Node[] }) => {
            for (const node of nodes) {
                try {
                    db.insert(schema.nodes).values({
                        name: node.name,
                        nodeType: node.nodeType,
                        metadata: JSON.stringify(node.metadata),
                    }).onConflictDoNothing().run();
                } catch (error) {
                    console.error(`[Sync] Error syncing added node "${node.name}" to SQLite:`, error);
                    if (error instanceof Error) {
                        console.error(`[Sync] Stack trace:`, error.stack);
                    }
                }
            }
        };
        this.graphOperations.on('afterAddNodes', afterAddNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterAddNodes', afterAddNodesHandler));

        const afterUpdateNodesHandler = async ({ nodes }: { nodes: Partial<Node>[] }) => {
            for (const node of nodes) {
                if (!node.name) continue;
                try {
                    // 1. Sync with SQLite
                    db.update(schema.nodes)
                        .set({
                            ...(node.nodeType && { nodeType: node.nodeType }),
                            ...(node.metadata && { metadata: JSON.stringify(node.metadata) }),
                            updatedAt: new Date()
                        })
                        .where(eq(schema.nodes.name, node.name))
                        .run();

                    // 2. Sync with Vector Store (if nodeType changed)
                    if (node.nodeType) {
                        // Check if node has an embedding
                        const nodeEmbedding = db.select()
                            .from(schema.embeddings)
                            .where(eq(schema.embeddings.nodeName, node.name))
                            .get();

                        if (nodeEmbedding) {
                            // Since LanceDB doesn't easily support metadata updates without re-inserting,
                            // we'd normally need the vector. But for now, we'll log this as a limitation
                            // or implement a basic update if LanceDB allows.
                            // UPDATE: We should ideally re-embed or just update the metadata in LanceDB.
                            console.error(`[Sync] Node type updated for "${node.name}". Vector metadata should be refreshed.`);
                        }
                    }
                } catch (error) {
                    console.error(`[Sync] Error syncing updated node "${node.name}":`, error);
                }
            }
        };
        this.graphOperations.on('afterUpdateNodes', afterUpdateNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterUpdateNodes', afterUpdateNodesHandler));

        const afterDeleteNodesHandler = async ({ nodeNames }: { nodeNames: string[] }) => {
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
        };
        this.graphOperations.on('afterDeleteNodes', afterDeleteNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterDeleteNodes', afterDeleteNodesHandler));

        // Edge synchronization
        const afterAddEdgesHandler = ({ edges }: { edges: Edge[] }) => {
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
        };
        this.graphOperations.on('afterAddEdges', afterAddEdgesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterAddEdges', afterAddEdgesHandler));

        const afterDeleteEdgesHandler = ({ edges }: { edges: Edge[] }) => {
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
        };
        this.graphOperations.on('afterDeleteEdges', afterDeleteEdgesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterDeleteEdges', afterDeleteEdgesHandler));
    }
}
