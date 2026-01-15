// src/core/errors/ConcurrencyError.ts

/**
 * Error thrown when an optimistic lock fails due to a version mismatch.
 * Indicates that the data was modified by another process since it was last read.
 */
export class ConcurrencyError extends Error {
    public readonly nodeName: string;
    public readonly expectedVersion: number;

    constructor(nodeName: string, expectedVersion: number) {
        super(`Concurrency conflict for node "${nodeName}": version ${expectedVersion} is stale.`);
        this.name = 'ConcurrencyError';
        this.nodeName = nodeName;
        this.expectedVersion = expectedVersion;
    }
}
