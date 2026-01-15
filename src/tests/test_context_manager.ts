
import { initDatabase, getDatabase, getSqliteInstance, closeDatabase } from '../infrastructure/database/index.js';
import { ContextManager } from '../core/context/ContextManager.js';

async function testContextManager() {
    console.log('Initializing Database...');
    initDatabase();

    // Clear existing data using raw SQL
    // getSqliteInstance returns the better-sqlite3 instance directly
    const sqlite = getSqliteInstance();
    try {
        sqlite.exec('DELETE FROM messages');
    } catch (e) {
        console.warn('Failed to clear messages table:', e);
    }

    // Check if we need schema types
    const db = getDatabase();
    const { messages } = await import('../infrastructure/database/schema.js');

    console.log('Inserting test messages...');
    await db.insert(messages).values([
        { role: 'user', content: 'Hello ReMem', tokenCount: 10 },
        { role: 'assistant', content: 'Hello User', tokenCount: 10 },
        { role: 'user', content: 'What is the plan?', tokenCount: 10 },
        { role: 'assistant', content: 'We are implementing ContextManager.', tokenCount: 20 }
    ]);

    const contextManager = new ContextManager();

    console.log('\n--- Testing getEffectiveContext ---');
    const context = await contextManager.getEffectiveContext('test_user');

    console.log('[L1 System]:', context.l1_system.substring(0, 50) + '...');
    console.log('[L2 Summary]:', context.l2_summary || '(Empty)');
    console.log('[L3 Conversation]:\n', context.l3_conversation);

    let success = true;

    if (!context.l3_conversation.includes('Hello ReMem')) {
        console.error('FAILURE: "Hello ReMem" not found in L3 context');
        success = false;
    }
    if (!context.l3_conversation.includes('ContextManager')) {
        console.error('FAILURE: "ContextManager" not found in L3 context');
        success = false;
    }

    // Verify ordering (Recent messages should be at the bottom, so later in string)
    const index1 = context.l3_conversation.indexOf('Hello ReMem');
    const index2 = context.l3_conversation.indexOf('ContextManager');
    if (index1 > index2) {
        console.error('FAILURE: Messages are not in chronological order');
        success = false;
    }

    if (success) {
        console.log('\nSUCCESS: Context retrieved correctly.');
    } else {
        console.error('\nTEST FAILED');
        process.exit(1);
    }

    closeDatabase();
}

testContextManager().catch(err => {
    console.error(err);
    process.exit(1);
});
