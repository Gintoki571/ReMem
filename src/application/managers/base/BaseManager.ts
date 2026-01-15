// src/application/managers/base/BaseManager.ts

import type { IStorage } from '@infrastructure/index.js';
import { SqliteStorage } from '@infrastructure/index.js';
import { ManagerFactory } from '@application/index.js';

/**
 * Base class that handles initialization and common functionality
 */
export abstract class BaseManager {
    protected readonly storage: IStorage;

    constructor(storage: IStorage = new SqliteStorage()) {
        this.storage = storage;
    }

    protected createManagers() {
        return {
            nodeManager: ManagerFactory.getNodeManager(this.storage),
            edgeManager: ManagerFactory.getEdgeManager(this.storage),
            metadataManager: ManagerFactory.getMetadataManager(this.storage),
            searchManager: ManagerFactory.getSearchManager(this.storage),
            transactionManager: ManagerFactory.getTransactionManager(this.storage)
        };
    }
}