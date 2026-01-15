// src/utils/retryLLM.ts

import { Logger } from '@core/logging/Logger.js';
import { CONFIG } from '@config/config.js';

export interface RetryOptions {
    maxRetries?: number;
    baseDelayMs?: number;
    onRetry?: (attempt: number, error: Error) => void;
}

/**
 * Retries an LLM operation with exponential backoff.
 * Recovers from transient network errors, rate limits, and timeouts.
 *
 * @param operation - The async function to retry.
 * @param options - Retry configuration.
 * @returns The result of the operation.
 * @throws The last error if all retries fail.
 */
export async function retryLLM<T>(
    operation: () => Promise<T>,
    options: RetryOptions = {}
): Promise<T> {
    const maxRetries = options.maxRetries ?? CONFIG.LLM.MAX_RETRIES;
    const baseDelayMs = options.baseDelayMs ?? CONFIG.LLM.RETRY_DELAY_MS;

    let lastError: Error = new Error('Unknown error');

    for (let attempt = 0; attempt <= maxRetries; attempt++) {
        try {
            return await operation();
        } catch (error) {
            lastError = error instanceof Error ? error : new Error(String(error));

            // Check if this is a retryable error
            const isRetryable = isRetryableError(lastError);

            if (!isRetryable || attempt >= maxRetries) {
                Logger.error('RetryLLM', `Non-retryable error or max retries reached: ${lastError.message}`);
                throw lastError;
            }

            const delay = baseDelayMs * Math.pow(2, attempt);
            Logger.warn('RetryLLM', `Attempt ${attempt + 1}/${maxRetries} failed: ${lastError.message}. Retrying in ${delay}ms...`);

            if (options.onRetry) {
                options.onRetry(attempt + 1, lastError);
            }

            await new Promise(resolve => setTimeout(resolve, delay));
        }
    }

    throw lastError;
}

/**
 * Determines if an error is retryable (transient).
 */
function isRetryableError(error: Error): boolean {
    const message = error.message.toLowerCase();

    // Network errors
    if (message.includes('econnreset') ||
        message.includes('econnrefused') ||
        message.includes('etimedout') ||
        message.includes('socket hang up') ||
        message.includes('network')) {
        return true;
    }

    // Rate limit errors (HTTP 429)
    if (message.includes('rate limit') ||
        message.includes('429') ||
        message.includes('too many requests')) {
        return true;
    }

    // Server errors (5xx)
    if (message.includes('500') ||
        message.includes('502') ||
        message.includes('503') ||
        message.includes('internal server error') ||
        message.includes('service unavailable')) {
        return true;
    }

    // Timeout errors
    if (message.includes('timeout') ||
        message.includes('timed out')) {
        return true;
    }

    return false;
}
