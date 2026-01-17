import { ITransactionManager } from './interfaces/ITransactionManager.js';
import { getDatabase, getSqliteInstance } from '@infrastructure/database/index.js';
import type { IStorage } from '@infrastructure/index.js';
import { Logger } from '@core/logging/Logger.js';

import { Graph } from '@core/index.js';

/**
 * TransactionManager
 * Manages atomic operations and Sagas with proper nested transaction support.
 * Implements ITransactionManager to support ApplicationManager requirements.
 */
export class TransactionManager extends ITransactionManager {
    private inTransactionState: boolean = false;
    private transactionDepth: number = 0;
    private rollbackActions: Array<{ action: () => Promise<void>, description: string }> = [];
    private savepointNames: string[] = [];

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
     * This wraps stateful calls to ensure safety with proper nested transaction support.
     */
    async executeTransaction<T>(operation: (tx: any) => Promise<T>): Promise<T> {
        if (this.transactionDepth > 0) {
            // Nested transaction: use savepoint
            const savepointName = `sp_${this.transactionDepth}_${Date.now()}`;
            this.transactionDepth++;
            this.savepointNames.push(savepointName);
            
            try {
                await this.createSavepoint(savepointName);
                const result = await operation(getDatabase());
                await this.releaseSavepoint(savepointName);
                this.transactionDepth--;
                this.savepointNames.pop();
                return result;
            } catch (error) {
                await this.rollbackToSavepoint(savepointName);
                this.transactionDepth--;
                this.savepointNames.pop();
                throw error;
            }
        }

        // Outer transaction
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
        this.transactionDepth = 1;
        this.rollbackActions = [];
        this.savepointNames = [];
    }

    private async createSavepoint(name: string): Promise<void> {
        const sqlite = getSqliteInstance();
        sqlite.prepare(`SAVEPOINT ${name}`).run();
        Logger.debug('TransactionManager', `Created savepoint: ${name}`);
    }

    private async releaseSavepoint(name: string): Promise<void> {
        const sqlite = getSqliteInstance();
        sqlite.prepare(`RELEASE SAVEPOINT ${name}`).run();
        Logger.debug('TransactionManager', `Released savepoint: ${name}`);
    }

    private async rollbackToSavepoint(name: string): Promise<void> {
        const sqlite = getSqliteInstance();
        try {
            sqlite.prepare(`ROLLBACK TO SAVEPOINT ${name}`).run();
            Logger.debug('TransactionManager', `Rolled back to savepoint: ${name}`);
        } catch (error) {
            Logger.error('TransactionManager', `Failed to rollback to savepoint: ${name}`, error);
            throw error;
        }
    }

    async commit(): Promise<void> {
        if (!this.inTransactionState) {
            throw new Error('No transaction to commit');
        }
        const sqlite = getSqliteInstance();
        sqlite.prepare('COMMIT').run();
        this.inTransactionState = false;
        this.transactionDepth = 0;
        this.rollbackActions = [];
        this.savepointNames = [];
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
            this.transactionDepth = 0;
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
        this.savepointNames = [];
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
        // Return empty graph structure since Graph is an interface.
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
            Logger.error('TransactionManager', 'Saga Failed. Rolling back...', error);
            // Internal Saga Rollback
            for (let i = completedSteps.length - 1; i >= 0; i--) {
                const step = completedSteps[i];
                try {
                    if (step.compensate) {
                        await step.compensate(result);
                    }
                } catch (rollbackError) {
                    Logger.error('TransactionManager', `CRITICAL: Saga rollback failed for ${step.name}`, rollbackError);
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