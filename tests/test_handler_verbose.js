
import { handleAutoAddMemory } from './dist/integration/tools/handlers/autoMemoryHandler.js';
import { ApplicationManager } from './dist/application/managers/ApplicationManager.js';
import { CONFIG } from './dist/config/config.js';
import dotenv from 'dotenv';
import fs from 'fs';
dotenv.config();

async function testFullFlow() {
    console.log('--- TEST START ---');
    console.log('Path:', CONFIG.PATHS.MEMORY_FILE);

    const manager = new ApplicationManager();
    await manager.initialize?.(); // If it exists

    console.log('Calling handleAutoAddMemory...');
    const result = await handleAutoAddMemory(
        { text: "The dragon Smaug is in his cave." },
        manager
    );

    console.log('Result:', JSON.stringify(result, null, 2));

    if (fs.existsSync(CONFIG.PATHS.MEMORY_FILE)) {
        const content = fs.readFileSync(CONFIG.PATHS.MEMORY_FILE, 'utf-8');
        console.log('File content size:', content.length);
        console.log('File content:', content);
    } else {
        console.log('File does not exist!');
    }
}

testFullFlow().catch(e => {
    console.error('CRASH:', e);
});
