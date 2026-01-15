
import { initDatabase, getDatabase, getSqliteInstance, closeDatabase } from '../infrastructure/database/index.js';
import { GlobalMemoryHandler } from '../integration/tools/handlers/GlobalMemoryHandler.js';
// We do NOT import ApplicationManager to avoid circular deps / complex init
// import { ApplicationManager } from '../application/managers/ApplicationManager.js'; 
import { ContextManager } from '../core/context/ContextManager.js';

// Mock ApplicationManager to satisfy BaseToolHandler dependency
class MockApplicationManager {
    async addNodes(nodes: any[]) {
        console.log('[MockAppManager] Adding nodes:', nodes.length);
        const db = getDatabase();
        const { nodes: nodesTable } = await import('../infrastructure/database/schema.js');

        // Map Node interface to DB schema if needed
        const dbNodes = nodes.map(n => ({
            name: n.name,
            nodeType: n.nodeType,
            metadata: JSON.stringify(n.metadata) // n.metadata is string[] in Interface, but Handler sends [{stringified obj}] which is object in Node?
            // Wait, GlobalMemoryHandler sends: metadata: [ JSON.stringify(...) ] (array of strings)
            // Schema 'nodes' table has metadata: text (string).
            // Drizzle should handle it? NO. Schema is `text`.
            // We should store `JSON.stringify(metadataArray)`
        }));

        // Actually, let's look at GlobalMemoryHandler.ts again.
        // It calls knowledgeGraphManager.addNodes(nodesToAdd).
        // nodesToAdd is Node[]. Node interface has metadata: string[].
        // GlobalMemoryHandler sets metadata: [ JSON.stringify(...) ].
        // GraphManager.addNodes probably handles storage.

        // Simple insertion for test:
        for (const n of nodes) {
            await db.insert(nodesTable).values({
                name: n.name,
                nodeType: n.nodeType,
                metadata: JSON.stringify(n.metadata) // Store the array as a JSON string string
            });
        }
    }
}

async function testGlobalMemory() {
    try {
        console.log('[Test] Initializing Database...');
        initDatabase();

        console.log('[Test] Creating MockApplicationManager...');
        const appManager = new MockApplicationManager() as any;

        console.log('[Test] Creating GlobalMemoryHandler...');
        const handler = new GlobalMemoryHandler(appManager);

        console.log('[Test] Creating ContextManager...');
        const contextManager = new ContextManager();

        const sqlite = getSqliteInstance();
        try {
            console.log('[Test] Cleaning up...');
            sqlite.exec("DELETE FROM nodes WHERE node_type = 'global_fact'");
        } catch (e) { console.warn(e); }

        console.log('[Test] Adding global facts via Handler...');
        const result = await handler.handleTool('add_global_memory', {
            facts: [
                "User prefers TypeScript.",
                "User hates semicolons (just kidding)."
            ]
        });
        console.log('[Test] Tool Execution Result:', JSON.stringify(result, null, 2));

        console.log('[Test] Retrieving Context...');
        const context = await contextManager.getEffectiveContext('test_user');

        console.log('[L1] System Profile:\n', context.l1_system);

        if (context.l1_system.includes("User prefers TypeScript")) {
            console.log('\nSUCCESS: Global facts found in L1 context.');
        } else {
            console.error('\nFAILURE: Global facts missing.');
            process.exit(1);
        }
    } catch (error) {
        console.error('[Test] Fatal Error:', error);
        process.exit(1);
    } finally {
        console.log('[Test] Closing database...');
        try { closeDatabase(); } catch (e) { console.warn('Error closing DB:', e); }
    }
}

testGlobalMemory().catch(err => {
    console.error('[Test] Uncaught:', err);
    process.exit(1);
});
