// src/core/managers/implementations/NodeManager.ts

import { IManager } from './interfaces/IManager.js';
import { INodeManager } from './interfaces/INodeManager.js';
import { GraphValidator } from '@core/index.js';
import type { Node } from '@core/index.js';
import { retryWithBackoff } from '@utils/retryWithBackoff.js';
import { ConcurrencyError } from '@core/errors/index.js';
import { Logger } from '@core/logging/Logger.js';

/**
 * Implements node-related operations for the knowledge graph.
 * Includes adding, updating, deleting, and retrieving nodes.
 */
export class NodeManager extends IManager implements INodeManager {
    /**
     * Adds new nodes to the knowledge graph.
     */
    async addNodes(nodes: Node[]): Promise<Node[]> {
        try {
            this.emit('beforeAddNodes', { nodes });

            const graph = await this.storage.loadGraph();
            const newNodes: Node[] = [];

            for (const node of nodes) {
                GraphValidator.validateNodeProperties(node);
                GraphValidator.validateNodeDoesNotExist(graph, node.name);
                newNodes.push(node);
            }

            graph.nodes.push(...newNodes);
            await this.storage.saveGraph(graph);

            this.emit('afterAddNodes', { nodes: newNodes });
            return newNodes;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }

    /**
     * Updates existing nodes in the knowledge graph.
     */
    /**
     * Updates existing nodes in the knowledge graph with Optimistic Locking.
     * Uses retryWithBackoff for conflict resolution (Merge & Retry).
     */
    async updateNodes(nodes: Partial<Node>[]): Promise<Node[]> {
        try {
            this.emit('beforeUpdateNodes', { nodes });

            // Validate input
            for (const node of nodes) {
                if (!node.name) throw new Error('Update requires node name');
            }

            const names = nodes.map(n => n.name as string);

            const result = await retryWithBackoff(async () => {
                // 1. Fetch current state (Atomic Read)
                const currentNodes = await this.storage.loadNodes(names);
                const nodesToUpdate: Node[] = [];

                for (const update of nodes) {
                    const current = currentNodes.find(n => n.name === update.name);
                    if (!current) {
                        throw new Error(`Node not found: ${update.name}`);
                    }

                    // 2. Conflict Resolution: Merge Logic
                    // We merge the update into the current state.
                    // Important: We use the VERSION from DB (current.version) 
                    // to ensure the CAS (Compare-And-Swap) works.
                    const merged: Node = {
                        ...current,
                        ...update,
                        metadata: {
                            ...(current.metadata || {}),
                            ...(update.metadata || {})
                        },
                        // Ensure version is from DB to pass optimistic lock check
                        version: current.version
                    };
                    nodesToUpdate.push(merged);
                }

                // 3. Persist (Atomic Write)
                await this.storage.updateNodes(nodesToUpdate);
                return nodesToUpdate;
            }, {
                maxRetries: 3,
                shouldRetry: (err) => err instanceof ConcurrencyError,
                initialDelay: 50,
                jitter: true
            });

            this.emit('afterUpdateNodes', { nodes: result });
            return result;
        } catch (error) {
            // Dead Letter Queue (Simulated via Error Log for now)
            Logger.error('NodeManager', 'Update failed after retries. Payload sent to DLQ.', {
                payload: nodes,
                error: error instanceof Error ? error.message : error
            });

            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to update nodes: ${errorMessage}`);
        }
    }

    /**
     * Deletes nodes and their associated edges from the knowledge graph.
     */
    async deleteNodes(nodeNames: string[]): Promise<void> {
        try {
            GraphValidator.validateNodeNamesArray(nodeNames);
            this.emit('beforeDeleteNodes', { nodeNames });

            const graph = await this.storage.loadGraph();
            const initialNodeCount = graph.nodes.length;

            graph.nodes = graph.nodes.filter(node => !nodeNames.includes(node.name));
            graph.edges = graph.edges.filter(edge =>
                !nodeNames.includes(edge.from) && !nodeNames.includes(edge.to)
            );

            const deletedCount = initialNodeCount - graph.nodes.length;

            await this.storage.saveGraph(graph);

            this.emit('afterDeleteNodes', { deletedCount });
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }

    /**
     * Retrieves specific nodes from the knowledge graph by their names.
     */
    async getNodes(nodeNames: string[]): Promise<Node[]> {
        try {
            // Optimized: Use loadNodes(WHERE IN) instead of loadGraph(ALL)
            return await this.storage.loadNodes(nodeNames);
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }
}