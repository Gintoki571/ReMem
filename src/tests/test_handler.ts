// src/tests/test_handler.ts

import { ToolHandlerFactory } from '@integration/tools/handlers/ToolHandlerFactory.js';
import { ApplicationManager } from '@application/managers/ApplicationManager.js';

async function testHandlers() {
    console.log('--- Tool Handler Factory Test ---');
    const manager = new ApplicationManager();
    ToolHandlerFactory.initialize(manager);

    const testTools = [
        'add_nodes',
        'search_nodes',
        'query_sql_db',
        'list_modules'
    ];

    for (const tool of testTools) {
        try {
            const handler = ToolHandlerFactory.getHandler(tool);
            console.log(`✅ Handler found for '${tool}': ${handler.constructor.name}`);
        } catch (e: any) {
            console.log(`❌ Failed to find handler for '${tool}': ${e.message}`);
        }
    }
}

testHandlers().catch(console.error);
