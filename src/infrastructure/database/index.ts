import Database from 'better-sqlite3';
import { drizzle, BetterSQLite3Database } from 'drizzle-orm/better-sqlite3';
import { migrate } from 'drizzle-orm/better-sqlite3/migrator';
import * as schema from './schema.js';
import path from 'path';
import { CONFIG } from '@config/config.js';

// Database file path - stored in root data directory
const DB_PATH = path.join(CONFIG.PATHS.DATA_DIR, 'remem.db');

let db: BetterSQLite3Database<typeof schema> | null = null;
let sqlite: Database.Database | null = null;

/**
 * Initialize the SQLite database connection
 */
export function initDatabase(): BetterSQLite3Database<typeof schema> {
    if (db) return db;

    sqlite = new Database(DB_PATH);
    sqlite.pragma('journal_mode = WAL'); // Better performance for concurrent reads

    db = drizzle(sqlite, { schema });

    // Create tables if they don't exist
    sqlite.exec(`
        CREATE TABLE IF NOT EXISTS nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            node_type TEXT NOT NULL,
            metadata TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        
        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_node TEXT NOT NULL,
            to_node TEXT NOT NULL,
            edge_type TEXT NOT NULL,
            weight REAL DEFAULT 1.0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            FOREIGN KEY (from_node) REFERENCES nodes(name),
            FOREIGN KEY (to_node) REFERENCES nodes(name)
        );
        
        CREATE TABLE IF NOT EXISTS embeddings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_name TEXT NOT NULL,
            embedding_id TEXT NOT NULL,
            text_content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            FOREIGN KEY (node_name) REFERENCES nodes(name)
        );
        
        CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
        CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);
        CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_node);
        CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_node);
        CREATE INDEX IF NOT EXISTS idx_embeddings_node ON embeddings(node_name);

        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            token_count INTEGER,
            is_summarized INTEGER DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE TABLE IF NOT EXISTS global_state (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        
        CREATE INDEX IF NOT EXISTS idx_messages_summarized ON messages(is_summarized);
        CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
    `);

    console.error('[DB] SQLite database initialized at:', DB_PATH);
    return db;
}

/**
 * Get the database instance
 */
export function getDatabase(): BetterSQLite3Database<typeof schema> {
    if (!db) {
        return initDatabase();
    }
    return db;
}

/**
 * Get the raw SQLite instance (for raw queries)
 */
export function getSqliteInstance(): Database.Database {
    if (!sqlite) {
        initDatabase();
    }
    return sqlite!;
}

/**
 * Close the database connection
 */
export function closeDatabase(): void {
    if (sqlite) {
        sqlite.close();
        sqlite = null;
        db = null;
        console.error('[DB] Database connection closed');
    }
}

export { schema };
