// src/core/managers/implementations/MetadataManager.ts

import { IMetadataManager } from './interfaces/IMetadataManager.js';
import { IManager } from './interfaces/IManager.js';
import { GraphValidator } from '@core/index.js';
import type { Metadata, MetadataAddition, MetadataResult, MetadataDeletion } from '@core/index.js';

/**
 * Implements metadata-related operations for the knowledge graph.
 * Includes adding, deleting, and retrieving metadata associated with nodes.
 */
export class MetadataManager extends IManager implements IMetadataManager {
    /**
     * Adds metadata to existing nodes.
     */
    async addMetadata(metadata: MetadataAddition[]): Promise<MetadataResult[]> {
        try {
            this.emit('beforeAddMetadata', { metadata });

            const graph = await this.storage.loadGraph();
            const results: MetadataResult[] = [];

            for (const item of metadata) {
                GraphValidator.validateNodeExists(graph, item.nodeName);
                const node = graph.nodes.find(e => e.name === item.nodeName);

                if (!node!.metadata) {
                    node!.metadata = {};
                }

                const newMetadata: Record<string, unknown> = {};
                for (const [key, value] of Object.entries(item.contents)) {
                    // Only add if different or new (optional optimization, but good for tracking 'added')
                    if (node!.metadata[key] !== value) {
                        node!.metadata[key] = value;
                        newMetadata[key] = value;
                    }
                }

                results.push({
                    nodeName: item.nodeName,
                    addedMetadata: Object.keys(newMetadata) // Return keys of added/updated items
                });
            }

            await this.storage.saveGraph(graph);

            this.emit('afterAddMetadata', { results });
            return results;
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }

    /**
     * Deletes metadata from nodes.
     */
    async deleteMetadata(deletions: MetadataDeletion[]): Promise<void> {
        try {
            this.emit('beforeDeleteMetadata', { deletions });

            const graph = await this.storage.loadGraph();
            let deletedCount = 0;

            for (const deletion of deletions) {
                GraphValidator.validateNodeExists(graph, deletion.nodeName);
                const node = graph.nodes.find(e => e.name === deletion.nodeName);

                if (node && node.metadata) {
                    const initialKeys = Object.keys(node.metadata).length;

                    for (const key of deletion.keys) {
                        delete node.metadata[key];
                    }

                    deletedCount += initialKeys - Object.keys(node.metadata).length;
                }
            }

            await this.storage.saveGraph(graph);

            this.emit('afterDeleteMetadata', { deletedCount });
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }

    /**
     * Retrieves metadata for a specific node.
     */
    async getMetadata(nodeName: string): Promise<Metadata> {
        try {
            const graph = await this.storage.loadGraph();
            GraphValidator.validateNodeExists(graph, nodeName);
            const node = graph.nodes.find(e => e.name === nodeName);

            return node!.metadata || {};
        } catch (error) {
            const errorMessage = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(errorMessage);
        }
    }
}