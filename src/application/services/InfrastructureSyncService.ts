import { getDatabase, schema } from '@infrastructure/database/index.js';
import { deleteVectorsByNode } from '@infrastructure/vector/VectorManager.js';
import type { GraphOperations } from '@application/operations/GraphOperations.js';
import type { Node, Edge } from '@core/index.js';
import { eq, and } from 'drizzle-orm';
import { retryWithBackoff } from '@utils/retryWithBackoff.js';

/**
 * Service that synchronizes graph operations with secondary storage (SQLite, Vector Store)
 * Uses retries to ensure eventual consistency.
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
        const afterAddNodesHandler = async ({ nodes }: { nodes: Node[] }) => {
            for (const node of nodes) {
                try {
                    await retryWithBackoff(async () => {
                        db.insert(schema.nodes).values({
                            name: node.name,
                            nodeType: node.nodeType,
                            metadata: node.metadata || {},
                        }).onConflictDoNothing().run();
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to sync added node "${node.name}" after retries:`, error);
                }
            }
        };
        this.graphOperations.on('afterAddNodes', afterAddNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterAddNodes', afterAddNodesHandler));

        const afterUpdateNodesHandler = async ({ nodes }: { nodes: Partial<Node>[] }) => {
            for (const node of nodes) {
                if (!node.name) continue;
                try {
                    await retryWithBackoff(async () => {
                        // Fetch the current version before updating
                        const existingNode = db.select({ version: schema.nodes.version })
                            .from(schema.nodes)
                            .where(eq(schema.nodes.name, node.name!))
                            .get();

                        if (!existingNode) {
                            console.warn(`[Sync] Node "${node.name}" not found in SQLite, skipping update sync.`);
                            return;
                        }

                        const currentVersion = existingNode.version;

                        // 1. Sync with SQLite using Conditional Update (Optimistic Locking)
                        const result = db.update(schema.nodes)
                            .set({
                                ...(node.nodeType && { nodeType: node.nodeType }),
                                ...(node.metadata && { metadata: node.metadata }),
                                version: currentVersion + 1, // Increment version
                                updatedAt: new Date()
                            })
                            .where(and(eq(schema.nodes.name, node.name!), eq(schema.nodes.version, currentVersion)))
                            .run();

                        if (result.changes === 0) {
                            // Version mismatch - throw ConcurrencyError to trigger retry
                            const { ConcurrencyError } = await import('@core/errors/index.js');
                            throw new ConcurrencyError(node.name!, currentVersion);
                        }

                        // 2. Sync with Vector Store (if nodeType changed)
                        if (node.nodeType) {
                            const nodeEmbedding = db.select()
                                .from(schema.embeddings)
                                .where(eq(schema.embeddings.nodeName, node.name!))
                                .get();

                            if (nodeEmbedding) {
                                console.warn(`[Sync] Node type updated for "${node.name}". Vector metadata should be refreshed.`);
                            }
                        }
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to sync updated node "${node.name}" after retries:`, error);
                }
            }
        };
        this.graphOperations.on('afterUpdateNodes', afterUpdateNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterUpdateNodes', afterUpdateNodesHandler));

        const afterDeleteNodesHandler = async ({ nodeNames }: { nodeNames: string[] }) => {
            for (const name of nodeNames) {
                try {
                    await retryWithBackoff(async () => {
                        // Sync with SQLite
                        db.delete(schema.nodes).where(eq(schema.nodes.name, name)).run();
                        db.delete(schema.edges).where(eq(schema.edges.fromNode, name)).run();
                        db.delete(schema.edges).where(eq(schema.edges.toNode, name)).run();

                        // Sync with Vector Store
                        await deleteVectorsByNode(name);
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to sync deleted node "${name}" after retries:`, error);
                }
            }
        };
        this.graphOperations.on('afterDeleteNodes', afterDeleteNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterDeleteNodes', afterDeleteNodesHandler));

        // Edge synchronization
        const afterAddEdgesHandler = async ({ edges }: { edges: Edge[] }) => {
            for (const edge of edges) {
                try {
                    await retryWithBackoff(async () => {
                        db.insert(schema.edges).values({
                            fromNode: edge.from,
                            toNode: edge.to,
                            edgeType: edge.edgeType,
                            weight: edge.weight ?? 1.0,
                        }).onConflictDoNothing().run();
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to sync added edge after retries:`, error);
                }
            }
        };
        this.graphOperations.on('afterAddEdges', afterAddEdgesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterAddEdges', afterAddEdgesHandler));

        const afterDeleteEdgesHandler = async ({ edges }: { edges: Edge[] }) => {
            for (const edge of edges) {
                try {
                    await retryWithBackoff(async () => {
                        db.delete(schema.edges)
                            .where(
                                and(
                                    eq(schema.edges.fromNode, edge.from),
                                    eq(schema.edges.toNode, edge.to),
                                    eq(schema.edges.edgeType, edge.edgeType)
                                )
                            )
                            .run();
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to sync deleted edge after retries:`, error);
                }
            }
        };
        this.graphOperations.on('afterDeleteEdges', afterDeleteEdgesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterDeleteEdges', afterDeleteEdgesHandler));
    }
}
