// src/application/managers/SearchManager.ts

import { ISearchManager } from './interfaces/ISearchManager.js';
import { IManager } from './interfaces/IManager.js';
import type { Graph, Node, Edge } from '@core/index.js';
import { GraphQueryEngine } from '@core/graph/GraphQueryEngine.js';
import type { IStorage } from '@infrastructure/index.js';

/**
 * Implements search-related operations for the knowledge graph.
 * Provides functionality for searching nodes and retrieving graph data.
 */
export class SearchManager extends IManager implements ISearchManager {
    private queryEngine: GraphQueryEngine;

    constructor(storage: IStorage) {
        super(storage);
        this.queryEngine = new GraphQueryEngine();
    }

    /**
     * Initializes the search manager and injects dependencies into Query Engine.
     */
    async initialize(): Promise<void> {
        try {
            await super.initialize();

            // DEPENDENCY INJECTION (CRIT-6 Fix):
            // Inject Vector Search and Analyzer into GraphQueryEngine to avoid circular imports in Core.
            // We import them here (Application Layer) where it acts as the composition root for this subsystem.

            // Dynamic import to ensure modules are loaded
            const { searchVectors } = await import('@infrastructure/vector/VectorManager.js');
            const { analyzer } = await import('@application/services/Analyzer.js');

            this.queryEngine.setDependencies(
                searchVectors,
                (text: string) => analyzer.generateEmbedding(text)
            );
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to initialize SearchManager: ${message}`);
        }
    }

    /**
     * Searches for nodes in the knowledge graph based on a query.
     * Uses GraphQueryEngine's "Vector -> Entry Point -> Subgraph" strategy.
     */
    async searchNodes(query: string, depth: number = 1): Promise<Graph> {
        try {
            this.emit('beforeSearch', { query });

            // Delegate to GraphQueryEngine (SQL Recursive CTE + Vector Search)
            const result = await this.queryEngine.findRelevantSubgraph(query, depth);

            this.emit('afterSearch', result);
            return result;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Search operation failed: ${message}`);
        }
    }

    /**
     * Retrieves specific nodes and their neighbors from the knowledge graph.
     * Delegates to GraphQueryEngine.findRelated for each node.
     */
    async openNodes(names: string[], depth: number = 1): Promise<Graph> {
        try {
            this.emit('beforeOpenNodes', { names });

            const allNodes = new Map<string, Node>();
            const allEdges: Edge[] = [];

            // GraphQueryEngine.findRelated is synchronous (better-sqlite3)
            for (const name of names) {
                const subgraph = this.queryEngine.findRelated(name, depth);
                subgraph.nodes.forEach((n: Node) => allNodes.set(n.name, n));
                subgraph.edges.forEach((e: Edge) => allEdges.push(e));
            }

            // Deduplicate edges
            const uniqueEdges = allEdges.filter((e, index, self) =>
                index === self.findIndex((t) => (
                    t.from === e.from && t.to === e.to && t.edgeType === e.edgeType
                ))
            );

            const result: Graph = {
                nodes: Array.from(allNodes.values()),
                edges: uniqueEdges
            };

            this.emit('afterOpenNodes', result);
            return result;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to open nodes: ${message}`);
        }
    }

    /**
     * Reads and returns the entire knowledge graph.
     * (Delegates to generic storage loadGraph as this is a dump, not a traversal)
     */
    async readGraph(limit?: number, offset?: number): Promise<Graph> {
        try {
            this.emit('beforeReadGraph', { limit, offset });
            const graph = await this.storage.loadGraph(limit, offset);
            this.emit('afterReadGraph', graph);
            return graph;
        } catch (error) {
            const message = error instanceof Error ? error.message : 'Unknown error occurred';
            throw new Error(`Failed to read graph: ${message}`);
        }
    }
}