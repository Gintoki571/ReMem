import { ITransactionManager } from './interfaces/ITransactionManager.js';
import { getDatabase, getSqliteInstance } from '@infrastructure/database/index.js';
import type { IStorage } from '@infrastructure/index.js';
import { Logger } from '@core/logging/Logger.js';

import { Graph } from '@core/index.js';

/**
 * TransactionManager
 * Manages atomic operations and Sagas.
 * Implements ITransactionManager to support ApplicationManager requirements.
 */
export class TransactionManager extends ITransactionManager {
    private inTransactionState: boolean = false;
    private rollbackActions: Array<{ action: () => Promise<void>, description: string }> = [];

    /**
     * Specialized emit wrapper that handles errors in listeners
     * and logs activity via structured Logger.
     */
    protected safeEmit(event: string, ...args: any[]): boolean {
        try {
            Logger.debug('TransactionManager', `Emitting event: ${event}`, { args });
            return this.emit(event, ...args);
        } catch (error) {
            Logger.error('TransactionManager', `Error in event listener for ${event}`, error);
            return false;
        }
    }

    // ApplicationManager passes storage, so we must accept it and pass to super
    constructor(storage: IStorage) {
        super(storage);
    }

    public async initialize(): Promise<void> {
        this.safeEmit('initialized', { manager: 'TransactionManager' });
    }

    /**
     * Executes a function within a SQLite transaction (Closure-based).
     * This wraps the stateful calls to ensure safety.
     */
    async executeTransaction<T>(operation: (tx: any) => Promise<T>): Promise<T> {
        if (this.inTransactionState) {
            // Already in transaction, just run operation
            return await operation(getDatabase());
        }

        await this.beginTransaction();
        try {
            const result = await operation(getDatabase());
            await this.commit();
            return result;
        } catch (error) {
            await this.rollback();
            throw error;
        }
    }

    async beginTransaction(): Promise<void> {
        if (this.inTransactionState) {
            throw new Error('Transaction already in progress');
        }
        const sqlite = getSqliteInstance();
        sqlite.prepare('BEGIN').run();
        this.inTransactionState = true;
        this.rollbackActions = [];
    }

    async commit(): Promise<void> {
        if (!this.inTransactionState) {
            throw new Error('No transaction to commit');
        }
        const sqlite = getSqliteInstance();
        sqlite.prepare('COMMIT').run();
        this.inTransactionState = false;
        this.rollbackActions = [];
    }

    async rollback(): Promise<void> {
        if (this.inTransactionState) {
            const sqlite = getSqliteInstance();
            try {
                sqlite.prepare('ROLLBACK').run();
            } catch (e) {
                Logger.error('TransactionManager', 'SQL Rollback failed', e);
            }
            this.inTransactionState = false;
        }

        // Execute compensating actions in reverse order
        Logger.info('TransactionManager', `Executing ${this.rollbackActions.length} rollback actions...`);
        for (let i = this.rollbackActions.length - 1; i >= 0; i--) {
            const { action, description } = this.rollbackActions[i];
            try {
                Logger.info('TransactionManager', `Rolling back: ${description}`);
                await action();
            } catch (e) {
                Logger.error('TransactionManager', `Rollback action failed: ${description}`, e);
            }
        }
        this.rollbackActions = [];
    }

    async addRollbackAction(action: () => Promise<void>, description: string): Promise<void> {
        this.rollbackActions.push({ action, description });
    }

    /**
     * Helper to wrap a block in a transaction (similar to executeTransaction but simpler signature)
     */
    async withTransaction<T>(operation: () => Promise<T>): Promise<T> {
        return this.executeTransaction(async () => operation());
    }

    isInTransaction(): boolean {
        return this.inTransactionState;
    }

    getCurrentGraph(): Graph {
        // Return emtpy graph structure since Graph is an interface.
        return { nodes: [], edges: [] };
    }

    /**
     * Executes a distributed saga (multi-step transaction).
     */
    async executeSaga<T>(steps: SagaStep<any>[]): Promise<T> {
        const completedSteps: SagaStep<any>[] = [];
        let result: any = null;

        try {
            for (const step of steps) {
                result = await step.execute(result);
                completedSteps.push(step);
            }
            return result;
        } catch (error) {
            console.error('[TransactionManager] Saga Failed. Rolling back...', error);
            // Internal Saga Rollback
            for (let i = completedSteps.length - 1; i >= 0; i--) {
                const step = completedSteps[i];
                try {
                    if (step.compensate) {
                        await step.compensate(result);
                    }
                } catch (rollbackError) {
                    console.error('[TransactionManager] CRITICAL: Saga rollback failed for', step.name, rollbackError);
                }
            }
            throw error;
        }
    }
}

export interface SagaStep<T> {
    name: string;
    execute: (input: any) => Promise<T>;
    compensate?: (result: T) => Promise<void>;
}


// We don't export a singleton instance anymore because ManagerFactory manages instances.
// However, legacy code might expect 'transactionManager' to be exported?
// ApplicationManager uses 'new TransactionManager(storage)'.
// autoMemoryHandler used 'transactionManager.executeTransaction'.
// If autoMemoryHandler imports the object, we should export a default instance OR update autoMemoryHandler to use ApplicationManager.
// autoMemoryHandler receives 'manager: ApplicationManager' in args!
// So it should use 'manager.transactionManager' or 'manager.withTransaction'.
// But 'manager.transactionManager' is private.
// 'manager.withTransaction' is public!

// So I should UPDATE autoMemoryHandler to use 'manager.withTransaction'
// instead of importing a singleton 'transactionManager'.

// But to keep build passing while I fix autoMemoryHandler, I might export a temporary singleton or mock.
// Actually autoMemoryHandler imports 'transactionManager' from '@application/managers/index'.
// I should remove that export from index or validly create it.
// I can export a singleton that lazily uses global storage? No.
// I should refactor autoMemoryHandler to use the passed 'manager'.
