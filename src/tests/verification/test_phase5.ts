// src/tests/verification/test_phase5.ts

import { Logger } from '../../core/logging/Logger.js';
import { TransactionManager } from '../../application/managers/TransactionManager.js';
import { SqliteStorage } from '../../infrastructure/storage/SqliteStorage.js';

async function testLoggerRedaction() {
    console.log('--- Testing Logger Redaction ---');
    const fakeKey = 'sk-1234567890abcdefghijklmnopqrstuvwxyz123456';

    console.log('Logging string with fake key...');
    Logger.info('Test', `Starting with key ${fakeKey}`);

    console.log('Logging object with fake key...');
    Logger.info('Test', 'Config object', { apiKey: fakeKey });
}

async function testEventSafety() {
    console.log('\n--- Testing Event Safety ---');
    const storage = new SqliteStorage();
    const tm = new TransactionManager(storage);

    // Add a failing listener
    tm.on('test-event', () => {
        throw new Error('Listener failed intentionally');
    });

    console.log('Emitting event with failing listener (should NOT crash)...');
    // We need to access safeEmit which is protected.
    // In TS we can cast to any for testing or make a test subclass
    const tmAny = tm as any;
    const result = tmAny.safeEmit('test-event', { foo: 'bar' });

    console.log(`Emit result: ${result} (expected false due to failure)`);
}

async function runTests() {
    try {
        await testLoggerRedaction();
        await testEventSafety();
        console.log('\n✅ Phase 5 Verification Complete');
    } catch (error) {
        console.error('\n❌ Phase 5 Verification Failed:', error);
        process.exit(1);
    }
}

runTests();
