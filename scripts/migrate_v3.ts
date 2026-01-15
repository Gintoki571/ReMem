
import { getSqliteInstance, closeDatabase } from '../src/infrastructure/database/index.js';

async function migrateV3() {
    console.log('[Migration] Starting V3 Migration (Optimistic Locking)...');

    const db = getSqliteInstance();

    try {
        // Check if column exists
        const tableInfo = db.prepare("PRAGMA table_info(nodes)").all() as any[];
        const hasVersion = tableInfo.some(col => col.name === 'version');

        if (!hasVersion) {
            console.log('[Migration] Adding "version" column to "nodes" table...');
            db.exec("ALTER TABLE nodes ADD COLUMN version INTEGER DEFAULT 1 NOT NULL");
            console.log('[Migration] Column added successfully.');
        } else {
            console.log('[Migration] "version" column already exists. Skipping.');
        }

    } catch (e) {
        console.error('[Migration] Failed:', e);
        process.exit(1);
    } finally {
        closeDatabase();
        console.log('[Migration] Database closed.');
    }
}

migrateV3();
