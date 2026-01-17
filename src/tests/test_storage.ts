
import { getDatabase, schema } from '@infrastructure/database/index.js';
import fs from 'fs';
import path from 'path';

// Manual JSON Write Test
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.join(__dirname, '../../');
const jsonPath = path.join(projectRoot, 'data/memory.json');

try {
    console.log('Testing JSON write to:', jsonPath);
    const data = { test: "success", timestamp: new Date().toISOString() };
    if (!fs.existsSync(path.dirname(jsonPath))) fs.mkdirSync(path.dirname(jsonPath), { recursive: true });
    fs.writeFileSync(jsonPath, JSON.stringify(data, null, 2));
    console.log('✅ JSON Write Success');
} catch (e) {
    console.error('❌ JSON Write Failed:', e);
}

// Manual SQLite Write Test
try {
    console.log('\nTesting SQLite write...');
    const db = getDatabase();
    const result = db.insert(schema.nodes).values({
        name: 'TEST_NODE_' + Date.now(),
        nodeType: 'test',
        metadata: { note: "manual_test", timestamp: new Date().toISOString() }
    }).onConflictDoNothing().run();

    console.log('✅ SQLite Write Success. Changes:', result.changes);

    const count = db.select().from(schema.nodes).all();
    console.log('   Total Nodes:', count.length);
} catch (e) {
    console.error('❌ SQLite Write Failed:', e);
}
