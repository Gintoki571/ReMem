// src/application/managers/SearchManager.ts

import { ISearchManager } from './interfaces/ISearchManager.js';
import { IManager } from './interfaces/IManager.js';
import type { Graph, Node, Edge } from '@core/index.js';

/**
 * Implements search-related operations for the knowledge graph.
 * Provides functionality for searching nodes and retrieving graph data.
 */
export class SearchManager extends IManager implements ISearchManager {
    /**
     * Searches for nodes in the knowledge graph based on a query.
     * Includes both matching nodes and their immediate neighbors.
     */
    async searchNodes(query: string, depth: number = 1): Promise<Graph> {
        try {
            this.emit('beforeSearch', { query });

            const graph = await this.storage.loadGraph();

            // Find directly matching nodes
            const startNodes = graph.nodes.filter(node =>
                node.name.toLowerCase().includes(query.toLowerCase()) ||
                node.nodeType.toLowerCase().includes(query.toLowerCase()) ||
                node.metadata.some(meta =>
                    meta.toLowerCase().includes(query.toLowerCase())
                )
            );

            const result = await this.bfsTraverse(startNodes.map(n => n.name), depth, graph);

            this.emit('afterSearch', result);
            return result;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Search operation failed: ${message}`);
        }
    }

    /**
     * Retrieves specific nodes and their neighbors from the knowledge graph.
     */
    async openNodes(names: string[], depth: number = 1): Promise<Graph> {
        try {
            this.emit('beforeOpenNodes', { names });

            const graph = await this.storage.loadGraph();

            const result = await this.bfsTraverse(names, depth, graph);

            this.emit('afterOpenNodes', result);
            return result;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to open nodes: ${message}`);
        }
    }

    /**
     * Internal BFS traversal to find nodes and edges up to a certain depth.
     */
    private async bfsTraverse(startNodeNames: string[], maxDepth: number, graph: Graph): Promise<Graph> {
        const resultNodes = new Map<string, Node>();
        const resultEdges = new Set<string>();
        const visited = new Set<string>();
        let queue: string[] = startNodeNames.filter(name =>
            graph.nodes.some(n => n.name === name)
        );

        // Track level to stop at maxDepth
        for (let depth = 0; depth <= maxDepth; depth++) {
            const nextLevel: string[] = [];

            // Add current queue nodes to results and mark as visited
            for (const name of queue) {
                if (!visited.has(name)) {
                    visited.add(name);
                    const node = graph.nodes.find(n => n.name === name);
                    if (node) resultNodes.set(name, node);
                }
            }

            // If we are not at the final depth, find neighbors
            if (depth < maxDepth) {
                for (const name of queue) {
                    const connections = graph.edges.filter(e => e.from === name || e.to === name);
                    for (const edge of connections) {
                        // Add edge to result (use a string key for set deduplication)
                        const edgeKey = `${edge.from}-${edge.to}-${edge.edgeType}`;
                        resultEdges.add(JSON.stringify(edge));

                        // Add target to next level if not visited
                        const neighbor = edge.from === name ? edge.to : edge.from;
                        if (!visited.has(neighbor)) {
                            nextLevel.push(neighbor);
                        }
                    }
                }
            } else {
                // Final level: still add edges between nodes we already have
                const currentNames = new Set(resultNodes.keys());
                graph.edges.forEach(edge => {
                    if (currentNames.has(edge.from) && currentNames.has(edge.to)) {
                        resultEdges.add(JSON.stringify(edge));
                    }
                });
            }

            queue = nextLevel;
            if (queue.length === 0) break;
        }

        return {
            nodes: Array.from(resultNodes.values()),
            edges: Array.from(resultEdges).map(e => JSON.parse(e))
        };
    }

    /**
     * Reads and returns the entire knowledge graph.
     */
    async readGraph(): Promise<Graph> {
        try {
            this.emit('beforeReadGraph', {});
            const graph = await this.storage.loadGraph();
            this.emit('afterReadGraph', graph);
            return graph;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to read graph: ${message}`);
        }
    }

    /**
     * Initializes the search manager.
     */
    async initialize(): Promise<void> {
        try {
            await super.initialize();
            // Add any search-specific initialization here
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to initialize SearchManager: ${message}`);
        }
    }
}