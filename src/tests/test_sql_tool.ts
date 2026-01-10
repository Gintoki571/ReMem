// src/tests/test_sql_tool.ts

import { SqlToolHandler } from '../integration/tools/handlers/SqlToolHandler.js';
import { ApplicationManager } from '../application/managers/ApplicationManager.js';

async function testSql() {
    console.log('--- SQL TOOL TEST ---');
    const handler = new SqlToolHandler(new ApplicationManager());

    // Test 1: Valid SELECT
    console.log('\n1. Testing VALID SELECT usage...');
    const result1 = await handler.handleTool('query_sql_db', {
        query: "SELECT name, nodeType FROM nodes LIMIT 2"
    });
    console.log('Result:', JSON.stringify(result1.toolResult.data || result1.toolResult.content, null, 2));

    // Test 2: Invalid DROP
    console.log('\n2. Testing INVALID DROP usage...');
    const result2 = await handler.handleTool('query_sql_db', {
        query: "DROP TABLE nodes"
    });
    const errorMsg = result2.toolResult.content[0].text;
    console.log('Blocked?', errorMsg.includes("Security Alert"));
    console.log('Message:', errorMsg);
}

testSql().catch(console.error);
