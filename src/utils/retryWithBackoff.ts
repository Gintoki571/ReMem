// src/utils/retryWithBackoff.ts

import { ConcurrencyError } from '@core/errors/index.js';
import { Logger } from '@core/logging/Logger.js';

export interface RetryOptions {
    maxRetries?: number;
    initialDelay?: number;
    maxDelay?: number;
    jitter?: boolean;
    shouldRetry?: (error: any) => boolean;
}

const DEFAULT_OPTIONS: Required<RetryOptions> = {
    maxRetries: 3,
    initialDelay: 100,
    maxDelay: 5000,
    jitter: true,
    shouldRetry: (error: any) => {
        // 1. Optimistic Locking
        if (error instanceof ConcurrencyError) return true;

        // 2. Database Locked (SQLite)
        if (error?.code === 'SQLITE_BUSY' ||
            (error instanceof Error && error.message.includes('database is locked'))) {
            return true;
        }

        // 3. Network / LLM Timeouts
        if (error?.code === 'ETIMEDOUT' ||
            error?.code === 'ECONNRESET' ||
            error?.type === 'request_timeout' ||
            error?.status === 429 || // Rate limit
            error?.status === 503 || // Service unavailable
            error?.status === 502) { // Bad gateway
            return true;
        }

        return false;
    }
};

/**
 * Retries an async operation with exponential backoff and jitter.
 * Handles ConcurrencyError, SQLITE_BUSY, and Network transients.
 */
export async function retryWithBackoff<T>(
    operation: () => Promise<T>,
    options: RetryOptions = {}
): Promise<T> {
    const config = { ...DEFAULT_OPTIONS, ...options };
    let lastError: any;

    for (let attempt = 0; attempt <= config.maxRetries; attempt++) {
        try {
            return await operation();
        } catch (error) {
            lastError = error;

            const isLastAttempt = attempt === config.maxRetries;
            const isRetryable = config.shouldRetry(error);

            if (isLastAttempt || !isRetryable) {
                // Determine reason for giving up
                // If it was retryable but we ran out of retries, we might want to log that.
                if (isRetryable && isLastAttempt) {
                    Logger.error('Retry', `Exhausted ${config.maxRetries} retries for operation. Last error: ${error instanceof Error ? error.message : String(error)}`);
                }
                throw error;
            }

            // Calculate delay with exponential backoff
            let delay = config.initialDelay * Math.pow(2, attempt);

            if (config.jitter) {
                // Add random jitter (0-20% of delay) to prevent thundering herd
                delay += Math.random() * (delay * 0.2);
            }

            // Cap delay
            delay = Math.min(delay, config.maxDelay);

            Logger.warn('Retry', `Operation failed, retrying in ${Math.round(delay)}ms (Attempt ${attempt + 1}/${config.maxRetries}). Error: ${error instanceof Error ? error.message : String(error)}`);

            await new Promise(resolve => setTimeout(resolve, delay));
        }
    }

    throw lastError;
}
