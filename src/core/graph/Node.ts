// src/core/graph/Node.ts

import type { Metadata } from '@core/index.js';

/**
 * Represents a node in the knowledge graph
 */
export interface Node {
    type: 'node';
    name: string;
    nodeType: string;
    metadata: Metadata;
    version?: number;
}