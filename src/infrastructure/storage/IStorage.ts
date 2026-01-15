// src/types/storage.ts

import type { Edge, Graph } from '@core/index.js';

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
}