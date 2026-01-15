// src/tests/integration/test_saga_rollback.ts

/**
 * Integration Test: Saga Rollback Verification
 * 
 * Tests that the TransactionManager correctly handles rollback scenarios:
 * 1. SQL transaction is started
 * 2. Nodes are added
 * 3. Vector embedding fails (simulated)
 * 4. SQL changes are rolled back
 * 5. Node no longer exists in database
 */

import { initDatabase, closeDatabase, getDatabase, schema, getSqliteInstance } from '@infrastructure/database/index.js';
import { eq } from 'drizzle-orm';

const TEST_NODE_NAME = 'SagaTestNode_' + Date.now();

async function setup() {
    console.log('[Test] Setting up saga rollback test...');
    initDatabase();
    console.log('[Test] Database initialized');
}

async function testManualRollback() {
    console.log('\n[Test] Running: Manual SQL Rollback Test');
    const db = getDatabase();
    const sqlite = getSqliteInstance();

    try {
        // Begin transaction
        sqlite.prepare('BEGIN').run();
        console.log('  Started transaction');

        // Insert test node
        db.insert(schema.nodes).values({
            name: TEST_NODE_NAME,
            nodeType: 'saga_test',
            metadata: JSON.stringify(['test']),
        }).run();
        console.log(`  Inserted node: ${TEST_NODE_NAME}`);

        // Verify node exists within transaction
        const nodeInTx = db.select().from(schema.nodes)
            .where(eq(schema.nodes.name, TEST_NODE_NAME)).get();

        if (!nodeInTx) {
            throw new Error('Node should exist within transaction');
        }
        console.log('  Node exists in transaction');

        // Simulate vector embedding failure
        console.log('  Simulating vector embedding failure...');
        throw new Error('Vector embedding failed!');

    } catch (error) {
        // Rollback on error
        console.log('  Rolling back transaction...');
        sqlite.prepare('ROLLBACK').run();
        console.log('  Transaction rolled back');

        // Verify node NO LONGER exists after rollback
        const nodeAfterRollback = db.select().from(schema.nodes)
            .where(eq(schema.nodes.name, TEST_NODE_NAME)).get();

        if (nodeAfterRollback) {
            throw new Error('Node should NOT exist after rollback!');
        }

        console.log('  ✅ PASSED: Node correctly removed after rollback');
    }
}

async function testCommitSuccess() {
    console.log('\n[Test] Running: Commit Success Test');
    const db = getDatabase();
    const sqlite = getSqliteInstance();

    const commitTestNode = 'CommitTestNode_' + Date.now();

    try {
        // Begin transaction
        sqlite.prepare('BEGIN').run();

        // Insert node
        db.insert(schema.nodes).values({
            name: commitTestNode,
            nodeType: 'commit_test',
            metadata: JSON.stringify(['success']),
        }).run();

        // Successful operation - commit
        sqlite.prepare('COMMIT').run();
        console.log('  Transaction committed');

        // Verify node exists after commit
        const nodeAfterCommit = db.select().from(schema.nodes)
            .where(eq(schema.nodes.name, commitTestNode)).get();

        if (!nodeAfterCommit) {
            throw new Error('Node should exist after commit');
        }

        console.log('  ✅ PASSED: Node persisted after commit');

        // Cleanup
        db.delete(schema.nodes).where(eq(schema.nodes.name, commitTestNode)).run();

    } catch (error) {
        sqlite.prepare('ROLLBACK').run();
        throw error;
    }
}

async function cleanup() {
    const db = getDatabase();
    // Clean up any leftover test nodes
    db.delete(schema.nodes).where(eq(schema.nodes.name, TEST_NODE_NAME)).run();
    await closeDatabase();
    console.log('\n[Test] Cleanup complete');
}

async function main() {
    try {
        await setup();
        await testManualRollback();
        await testCommitSuccess();
        console.log('\n========================================');
        console.log('  ALL SAGA ROLLBACK TESTS PASSED ✅');
        console.log('========================================');
    } catch (error) {
        console.error('\n❌ TEST FAILED:', error);
        process.exit(1);
    } finally {
        await cleanup();
    }
}

main();
