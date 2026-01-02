
import { handleAutoAddMemory } from './dist/integration/tools/handlers/autoMemoryHandler.js';
import { ApplicationManager } from './dist/application/managers/ApplicationManager.js';
import dotenv from 'dotenv';
dotenv.config();

async function testFullFlow() {
    console.log('Testing full Auto-Add flow (Handler + Storage)...');

    // Mock ApplicationManager
    // Note: This needs a real manager to test the real file-write logic
    // We can use a real instance since it's a test environment
    const manager = new ApplicationManager();
    await manager.initialize();

    const result = await handleAutoAddMemory(
        { text: "The dragon Smaug is in his cave." },
        manager
    );

    console.log('Handler Result:', JSON.stringify(result, null, 2));

    // Check if data was saved
    const nodes = await manager.searchNodes(""); // Should return all
    console.log('Nodes in JSON after tool:', nodes.map(n => n.name));
}

testFullFlow().catch(console.error);
