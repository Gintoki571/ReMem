// src/application/managers/interfaces/ITransactionManager.ts

import type {Graph} from '@core/index.js';
import {IManager} from './IManager.js';

export interface RollbackAction {
    action: () => Promise<void>;
    description: string;
}

export abstract class ITransactionManager extends IManager {
    abstract beginTransaction(): Promise<void>;

    abstract addRollbackAction(action: () => Promise<void>, description: string): Promise<void>;

    abstract commit(): Promise<void>;

    abstract rollback(): Promise<void>;

    abstract getCurrentGraph(): Graph;

    abstract isInTransaction(): boolean;

    abstract withTransaction<T>(operation: () => Promise<T>): Promise<T>;
}
