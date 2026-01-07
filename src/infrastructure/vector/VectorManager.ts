import { connect, Table, Connection } from '@lancedb/lancedb';
import path from 'path';
import { CONFIG } from '@config/config.js';

// LanceDB storage path
const LANCEDB_PATH = path.join(CONFIG.PATHS.DATA_DIR, 'lancedb');

interface VectorRecord {
    id: string;
    text: string;
    vector: number[];
    nodeName: string;
    nodeType: string;
    metadata?: string;
    [key: string]: unknown; // Index signature for Record<string, unknown> compatibility
}

let db: Connection | null = null;
let table: Table | null = null;

const TABLE_NAME = 'memory_vectors';

/**
 * Initialize LanceDB connection and table
 */
export async function initVectorStore(): Promise<void> {
    if (db) return;

    db = await connect(LANCEDB_PATH);

    // Check if table exists, create if not
    const tables = await db.tableNames();

    if (!tables.includes(TABLE_NAME)) {
        // Create table with initial empty schema (LanceDB needs at least one record)
        // We'll create it on first insert
        console.error('[VectorDB] LanceDB initialized, table will be created on first insert');
    } else {
        table = await db.openTable(TABLE_NAME);
        console.error('[VectorDB] LanceDB table opened:', TABLE_NAME);
    }
}

/**
 * Add a vector to the store
 */
export async function addVector(record: VectorRecord): Promise<void> {
    if (!db) await initVectorStore();

    if (!table) {
        // Create table with first record - cast for LanceDB compatibility
        table = await db!.createTable(TABLE_NAME, [record as Record<string, unknown>]);
        console.error('[VectorDB] Created table with first record');
    } else {
        await table.add([record as Record<string, unknown>]);
    }
}

/**
 * Search for similar vectors
 */
export async function searchVectors(
    queryVector: number[],
    limit: number = 5
): Promise<VectorRecord[]> {
    if (!db) await initVectorStore();
    if (!table) {
        console.error('[VectorDB] No vectors in store yet');
        return [];
    }

    const results = await table
        .search(queryVector)
        .limit(limit)
        .toArray();

    return results as VectorRecord[];
}

/**
 * Delete vectors by node name
 */
export async function deleteVectorsByNode(nodeName: string): Promise<void> {
    if (!db) await initVectorStore();
    if (!table) return;

    await table.delete(`nodeName = '${nodeName}'`);
}

/**
 * Get all vectors (for debugging/export)
 */
export async function getAllVectors(): Promise<VectorRecord[]> {
    if (!db) await initVectorStore();
    if (!table) return [];

    const results = await table.query().toArray();
    return results as VectorRecord[];
}

/**
 * Close the vector store connection
 */
export async function closeVectorStore(): Promise<void> {
    if (db) {
        // LanceDB doesn't have explicit close, but we can reset references
        db = null;
        table = null;
        console.error('[VectorDB] Vector store connection reset');
    }
}

export type { VectorRecord };
