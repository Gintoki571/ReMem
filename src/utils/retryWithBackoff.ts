// src/utils/retryWithBackoff.ts

import { ConcurrencyError } from '@core/errors/index.js';

/**
 * Retries an async operation with exponential backoff.
 * Specifically designed for handling ConcurrencyError in optimistic locking scenarios.
 *
 * @param operation - The async function to retry.
 * @param maxRetries - Maximum number of retry attempts.
 * @param baseDelayMs - Initial delay in milliseconds (doubled on each retry).
 * @returns The result of the operation.
 * @throws The last error if all retries fail.
 */
export async function retryWithBackoff<T>(
    operation: () => Promise<T>,
    maxRetries: number = 3,
    baseDelayMs: number = 100
): Promise<T> {
    let lastError: Error | null = null;

    for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
            return await operation();
        } catch (error) {
            if (error instanceof ConcurrencyError) {
                lastError = error;
                const delay = baseDelayMs * Math.pow(2, attempt);
                console.warn(`[Retry] Concurrency conflict for "${error.nodeName}", attempt ${attempt + 1}/${maxRetries}. Retrying in ${delay}ms...`);
                await new Promise(resolve => setTimeout(resolve, delay));
            } else {
                // Non-retryable error, re-throw immediately
                throw error;
            }
        }
    }

    // All retries failed
    throw lastError ?? new Error('Retry failed with unknown error');
}
