
import { getDatabase, schema } from '@infrastructure/database/index.js';
import fs from 'fs';
import path from 'path';

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
