
import Database from 'better-sqlite3';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Adjust path as needed. We are in ReMem root essentially for this script context if we place it there
// logic: ReMem/ReMem_Engine/data/remem.db
const dbPath = path.join('c:/Users/Bindesh Kandel/ReMem/ReMem_Engine/data/remem.db');

try {
    const db = new Database(dbPath, { readonly: true });
    const nodes = db.prepare('SELECT * FROM nodes').all();

    // Parse metadata for display if it's a string
    const formatted = nodes.map(n => {
        try {
            return {
                ...n,
                metadata: typeof n.metadata === 'string' ? JSON.parse(n.metadata) : n.metadata
            };
        } catch (e) {
            return n;
        }
    });

    console.log(JSON.stringify(formatted, null, 2));
} catch (error) {
    console.error('Error reading DB:', error);
}
