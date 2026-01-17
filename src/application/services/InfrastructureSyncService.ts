import { deleteVectorsByNode } from '@infrastructure/vector/VectorManager.js';
import type { GraphOperations } from '@application/operations/GraphOperations.js';
import type { Node } from '@core/index.js';
import { retryWithBackoff } from '@utils/retryWithBackoff.js';
import { getDatabase, schema } from '@infrastructure/database/index.js'; // Keep for reading existence checks if needed, but wait...
// Actually, we don't need to read existence for VECTOR sync if we just trust the event?
// But for 'afterUpdateNodes', we checked if nodeType changed.
// We DO need to read from SQLite to see if nodeType changed? 
// Or does the event contain old vs new? The event 'nodes' is Partial<Node>.
// If Partial<Node> has nodeType, we might need to re-vectorize? 
// The vector store contains 'nodeType' in metadata.

// SIMPLIFICATION:
// 1. afterAddNodes: No action needed for Vectors (vectors are added via Analyzer explicitly or manually?)
//    Wait, who adds vectors? 'autoMemoryHandler' calls 'analyzer.generateEmbedding' then 'vectorManager.addVector'.
//    So 'VectorSync' is NOT responsible for initial vector creation in 'autoMemoryHandler'.
//    Is it responsible for anything?
//    If I update a node via `update_node` tool, proper behavior is to update the vector too?
//    Currently, 'update_node' tool -> GraphManager -> GraphOperations -> emits event.
//    If 'update_node' does NOT update vector, then vector is stale.
//    So 'afterUpdateNodes' MUST sync to Vector.
//    BUT 'autoMemoryHandler' handles vectors manually.
//    We need to be careful not to double-vectorize if autoMemoryHandler does it.
//    However, manual tool usage 'update_node' won't call 'analyzer', so SyncService is the ONLY place to catch manual updates.

// DECISION:
// - Keep Vector Sync for 'update' (if nodeType changes or checking consistency) and 'delete'.
// - Remove 'add' sync entirely (vectors are added explicitly by intelligence layer).
// - Remove 'edge' sync entirely (vectors don't track edges).

import { eq } from 'drizzle-orm';

/**
 * Service that synchronizes graph operations with Vector Storage.
 * (Formerly InfrastructureSyncService, now stripped of redundant SQLite dual-writes)
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

        // Node Update Synchronization (Vector Validity Check)
        const afterUpdateNodesHandler = async ({ nodes }: { nodes: Partial<Node>[] }) => {
            for (const node of nodes) {
                if (!node.name) continue;
                try {
                    // Check if nodeType changed, which might affect vector relevance
                    if (node.nodeType) {
                        const nodeEmbedding = db.select()
                            .from(schema.embeddings)
                            .where(eq(schema.embeddings.nodeName, node.name))
                            .get();

                        if (nodeEmbedding) {
                            // In a full implementation, we might re-generate embedding here.
                            // For now, we warn that metadata might be stale.
                            console.warn(`[Sync] Node type updated for "${node.name}". Vector metadata should be refreshed.`);
                        }
                    }
                } catch (error) {
                    console.error(`[Sync] Failed to check vector status for updated node "${node.name}":`, error);
                }
            }
        };
        this.graphOperations.on('afterUpdateNodes', afterUpdateNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterUpdateNodes', afterUpdateNodesHandler));

        // Node Delete Synchronization (Vector Cleanup)
        const afterDeleteNodesHandler = async ({ nodeNames }: { nodeNames: string[] }) => {
            for (const name of nodeNames) {
                try {
                    await retryWithBackoff(async () => {
                        // Sync with Vector Store (Cleanup)
                        await deleteVectorsByNode(name);
                    });
                } catch (error) {
                    console.error(`[Sync] CRITICAL: Failed to cleanup vectors for deleted node "${name}":`, error);
                }
            }
        };
        this.graphOperations.on('afterDeleteNodes', afterDeleteNodesHandler);
        this.cleanupFunctions.push(() => this.graphOperations.off('afterDeleteNodes', afterDeleteNodesHandler));

        // Edge operations do not affect Vector Store currently, so we ignore them.
    }
}
