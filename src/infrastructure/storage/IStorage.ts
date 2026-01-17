// src/types/storage.ts

import type { Edge, Graph, Node } from '@core/index.js';

/**
 * Edge indexing structure
 */
export interface EdgeIndex {
    byFrom: Map<string, Set<string>>;
    byTo: Map<string, Set<string>>;
    byType: Map<string, Set<string>>;
}

/**
 * Storage interface for graph operations
 */
export interface IStorage {
    loadGraph(limit?: number, offset?: number): Promise<Graph>;

    saveGraph(graph: Graph): Promise<void>;

    loadEdgesByIds(edgeIds: string[]): Promise<Edge[]>;

    /**
     * Loads specific nodes by their names.
     */
    loadNodes(names: string[]): Promise<Node[]>;

    /**
     * Updates specific nodes with optimistic locking support.
     * @throws ConcurrencyError if version mismatch.
     */
    updateNodes(nodes: Node[]): Promise<void>;
}