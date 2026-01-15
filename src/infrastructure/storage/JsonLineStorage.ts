// src/core/storage/JsonLineStorage.ts

import { promises as fs } from 'fs';
import path from 'path';
import { CONFIG } from '@config/config.js';
import type { IStorage } from './IStorage.js';
import type { Edge, Graph } from '@core/index.js';
import { randomBytes } from 'crypto';

/**
 * Handles persistent storage of the knowledge graph using a JSON Lines file format.
 * Uses atomic write-rename pattern to prevent corruption from concurrent writes.
 */
export class JsonLineStorage implements IStorage {
    private initialized: boolean;
    private writeLock: Promise<void> = Promise.resolve();

    constructor() {
        this.initialized = false;
    }

    /**
     * Ensures the storage file and directory exist
     */
    private async ensureStorageExists(): Promise<void> {
        if (this.initialized) {
            return;
        }

        const MEMORY_FILE_PATH = CONFIG.PATHS.MEMORY_FILE;
        const dir = path.dirname(MEMORY_FILE_PATH);

        try {
            // Check if directory exists, create if it doesn't
            try {
                await fs.access(dir);
            } catch {
                await fs.mkdir(dir, { recursive: true });
            }

            // Check if file exists, create if it doesn't
            try {
                await fs.access(MEMORY_FILE_PATH);
            } catch {
                await fs.writeFile(MEMORY_FILE_PATH, '');
            }

            this.initialized = true;
        } catch (error) {
            console.error('Error initializing storage:', error);
            throw new Error('Failed to initialize storage');
        }
    }

    /**
     * Loads the entire knowledge graph from storage and builds the edge indices.
     */
    async loadGraph(): Promise<Graph> {
        await this.ensureStorageExists();

        try {
            const MEMORY_FILE_PATH = CONFIG.PATHS.MEMORY_FILE;
            const data = await fs.readFile(MEMORY_FILE_PATH, "utf-8");
            const lines = data.split("\n").filter(line => line.trim() !== "");


            const graph: Graph = { nodes: [], edges: [] };

            for (const line of lines) {
                try {
                    const item = JSON.parse(line);
                    if (item.type === "node") {
                        graph.nodes.push(item);
                    } else if (item.type === "edge") {
                        graph.edges.push(item);
                    }
                } catch (parseError) {
                    console.error('Error parsing line:', line, parseError);
                }
            }

            return graph;
        } catch (error) {
            if (error instanceof Error && 'code' in error && error.code === "ENOENT") {
                return { nodes: [], edges: [] };
            }
            throw error;
        }
    }

    /**
     * Saves the entire knowledge graph to storage.
     * Uses atomic write-rename pattern to prevent corruption.
     */
    async saveGraph(graph: Graph): Promise<void> {
        await this.ensureStorageExists();

        // Serialize write operations to prevent race conditions
        this.writeLock = this.writeLock.then(async () => {
            const MEMORY_FILE_PATH = CONFIG.PATHS.MEMORY_FILE;
            const tempPath = `${MEMORY_FILE_PATH}.tmp.${randomBytes(8).toString('hex')}`;

            try {
                const processedEdges = graph.edges.map(edge => ({
                    ...edge,
                    type: 'edge'
                }));

                const lines = [
                    ...graph.nodes.map(node => JSON.stringify({ ...node, type: 'node' })),
                    ...processedEdges.map(edge => JSON.stringify(edge))
                ];

                // Write to temp file first
                await fs.writeFile(tempPath, lines.join("\n") + (lines.length > 0 ? "\n" : ""));
                
                // Atomic rename (overwrites destination)
                await fs.rename(tempPath, MEMORY_FILE_PATH);
            } catch (error) {
                // Clean up temp file on error
                try {
                    await fs.unlink(tempPath);
                } catch {
                    // Ignore cleanup errors
                }
                throw error;
            }
        });

        await this.writeLock;
    }

    /**
     * Loads specific edges by their IDs from storage.
     */
    async loadEdgesByIds(edgeIds: string[]): Promise<Edge[]> {
        const graph = await this.loadGraph();
        const edgeMap = new Map(
            graph.edges.map(edge => [this.generateEdgeId(edge), edge])
        );

        return edgeIds
            .map(id => edgeMap.get(id))
            .filter((edge): edge is Edge => edge !== undefined);
    }

    /**
     * Generates a unique ID for an edge based on its properties.
     */
    private generateEdgeId(edge: Edge): string {
        return `${edge.from}|${edge.to}|${edge.edgeType}`;
    }
}