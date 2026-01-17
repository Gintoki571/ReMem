import { connect, Table, Connection } from '@lancedb/lancedb';
import path from 'path';
import fs from 'fs';
import { Mutex } from 'async-mutex';
import { CONFIG } from '@config/config.js';
import { Logger } from '@core/logging/Logger.js';

// LanceDB storage path
const LANCEDB_PATH = path.join(CONFIG.PATHS.DATA_DIR, 'lancedb');

// Security: Whitelist pattern for node names (prevents SQL injection)
const NODE_NAME_REGEX = /^[a-zA-Z0-9_-]{1,200}$/;

class ValidationError extends Error {
    constructor(message: string) {
        super(message);
        this.name = 'ValidationError';
    }
}

/** Validate node name against whitelist to prevent injection attacks */
function validateNodeName(nodeName: string): void {
    if (!NODE_NAME_REGEX.test(nodeName)) {
        throw new ValidationError(
            `Invalid node name: '${nodeName.substring(0, 50)}'. ` +
            `Must match pattern: ${NODE_NAME_REGEX.source}`
        );
    }
}

interface VectorRecord {
    id: string;
    text: string;
    vector: number[];
    nodeName: string;
    nodeType: string;
    metadata?: Record<string, unknown>;
    [key: string]: unknown; // Index signature for Record<string, unknown> compatibility
}

let db: Connection | null = null;
let table: Table | null = null;
const initMutex = new Mutex(); // Prevents race condition during initialization

const TABLE_NAME = 'memory_vectors';

/**
 * Initialize LanceDB connection and table
 * Uses mutex to prevent race condition when multiple calls try to init simultaneously
 */
export async function initVectorStore(): Promise<void> {
    const release = await initMutex.acquire();
    try {
        if (db) return; // Already initialized

        try {
            await attemptConnection();
        } catch (error) {
            Logger.error('VectorDB', `Initialization failed: ${error instanceof Error ? error.message : 'Unknown'}. Attempting recovery...`);
            await recoverVectorStore();
            await attemptConnection();
        }
    } finally {
        release();
    }
}

async function attemptConnection(): Promise<void> {
    // Ensure directory exists
    if (!fs.existsSync(LANCEDB_PATH)) {
        fs.mkdirSync(LANCEDB_PATH, { recursive: true });
    }

    db = await connect(LANCEDB_PATH);

    // Check if table exists, create if not
    const tables = await db.tableNames();

    if (!tables.includes(TABLE_NAME)) {
        Logger.info('VectorDB', 'LanceDB initialized, table will be created on first insert');
    } else {
        table = await db.openTable(TABLE_NAME);
        // Verify table is readable
        await table.countRows();
        Logger.info('VectorDB', `LanceDB table opened: ${TABLE_NAME}`);
    }
}

async function recoverVectorStore(): Promise<void> {
    try {
        if (fs.existsSync(LANCEDB_PATH)) {
            const backupPath = `${LANCEDB_PATH}_corrupt_${Date.now()}`;
            fs.renameSync(LANCEDB_PATH, backupPath);
            Logger.warn('VectorDB', `Corrupted vector store moved to ${backupPath}`);
        }
        // Reset state
        db = null;
        table = null;
        // Recreate directory
        fs.mkdirSync(LANCEDB_PATH, { recursive: true });
    } catch (error) {
        Logger.error('VectorDB', `Recovery failed: ${error}`);
        throw error; // Critical failure if we can't even move the folder
    }
}

/**
 * Add a vector to the store
 * @throws ValidationError if nodeName contains invalid characters
 */
export async function addVector(record: VectorRecord): Promise<void> {
    // Security: Validate node name before any DB operation
    validateNodeName(record.nodeName);

    if (!db) await initVectorStore();

    if (table) {
        // Safe delete: nodeName is now validated, no injection possible
        await table.delete(`nodeName = '${record.nodeName}'`);
        await table.add([record as Record<string, unknown>]);
    } else {
        // Create table with first record
        table = await db!.createTable(TABLE_NAME, [record as Record<string, unknown>]);
        Logger.info('VectorDB', 'Created table with first record');
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
        Logger.warn('VectorDB', 'No vectors in store yet');
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
 * @throws ValidationError if nodeName contains invalid characters
 */
export async function deleteVectorsByNode(nodeName: string): Promise<void> {
    // Security: Validate node name before any DB operation
    validateNodeName(nodeName);

    if (!db) await initVectorStore();
    if (!table) return;

    // Safe delete: nodeName is now validated
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
        Logger.info('VectorDB', 'Vector store connection reset');
    }
}

export type { VectorRecord };
