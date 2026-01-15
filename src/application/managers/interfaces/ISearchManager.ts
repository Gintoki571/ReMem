// src/application/managers/interfaces/ISearchManager.ts

import { IManager } from './IManager.js';
import type { Graph } from '@core/index.js';

/**
 * Interface for search-related operations in the knowledge graph.
 * Defines the contract for searching nodes based on queries and retrieving specific nodes.
 */
export interface ISearchManager extends IManager {
    /**
     * Searches for nodes in the knowledge graph based on a query.
     */
    searchNodes(query: string, depth?: number): Promise<Graph>;

    /**
     * Retrieves specific nodes from the knowledge graph by their names.
     */
    openNodes(names: string[], depth?: number): Promise<Graph>;

    /**
     * Reads and returns the entire knowledge graph.
     */
    readGraph(limit?: number, offset?: number): Promise<Graph>;
}