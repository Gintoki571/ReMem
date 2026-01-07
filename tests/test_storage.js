
import { getDatabase, schema } from './dist/infrastructure/database/index.js';
import fs from 'fs';

// Manual JSON Write Test
const jsonPath = './dist/data/memory.json';
try {
    console.log('Testing JSON write to:', jsonPath);
    const data = { test: "success", timestamp: new Date().toISOString() };
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
        metadata: '["manual_test"]'
    }).onConflictDoNothing().run();

    console.log('✅ SQLite Write Success. Changes:', result.changes);

    const count = db.select().from(schema.nodes).all();
    console.log('   Total Nodes:', count.length);
} catch (e) {
    console.error('❌ SQLite Write Failed:', e);
}
