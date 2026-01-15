
import { initDatabase, getDatabase, getSqliteInstance, closeDatabase, schema } from '../infrastructure/database/index.js';
import { ContextManager } from '../core/context/ContextManager.js';
import { analyzer } from '../application/services/Analyzer.js';

// Mock Analyzer to avoid real LLM calls
analyzer.summarizeMessages = async (msgs, prev) => {
    return `[Summary] Previous: ${prev ? prev.substring(0, 10) : ''}... + New: ${msgs.length} messages.`;
};

async function testContextLoop() {
    try {
        console.log('[Test] Initializing DB...');
        initDatabase();
        const db = getDatabase();
        const cm = new ContextManager();

        // 1. Setup: Clear relevant tables
        getSqliteInstance().exec("DELETE FROM messages");
        getSqliteInstance().exec("DELETE FROM nodes WHERE name = 'user_loop_test_summary'");

        // 2. Insert dummy messages
        console.log('[Test] Seeding messages...');
        const messages = [];
        for (let i = 0; i < 5; i++) {
            await db.insert(schema.messages).values({
                role: i % 2 === 0 ? 'user' : 'assistant',
                content: `Message ${i}`,
                isSummarized: false
            });
        }

        // 3. Trigger Compaction
        console.log('[Test] Running compactContext...');
        await cm.compactContext('user_loop_test');

        // 4. Verify Summary Node Created
        const summaryNode = await db.query.nodes.findFirst({
            where: (nodes, { eq }) => eq(nodes.name, 'user_loop_test_summary')
        });

        if (!summaryNode) {
            throw new Error('Summary node was not created.');
        }
        console.log('[Test] Summary Node Content:', summaryNode.metadata);

        // 5. Verify Messages Marked Summarized
        const remainingUnsummarized = await db.query.messages.findMany({
            where: (m, { eq }) => eq(m.isSummarized, false)
        });

        if (remainingUnsummarized.length === 0) {
            console.log('SUCCESS: All messages summarized.');
        } else {
            console.error(`FAILURE: ${remainingUnsummarized.length} messages left unsummarized.`);
            process.exit(1);
        }

    } catch (e) {
        console.error('[Test] Failed:', e);
        process.exit(1);
    } finally {
        closeDatabase();
    }
}

testContextLoop();
