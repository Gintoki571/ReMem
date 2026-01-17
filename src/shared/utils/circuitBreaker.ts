/**
 * Circuit Breaker pattern implementation for external API calls
 * Prevents cascading failures when external services are unavailable
 */

export interface CircuitBreakerOptions {
    failureThreshold: number;    // Number of failures before opening circuit
    resetTimeout: number;        // Time in milliseconds to wait before trying again
    monitoringPeriod: number;    // Time window to count failures
}

export enum CircuitState {
    CLOSED = 'CLOSED',     // Normal operation
    OPEN = 'OPEN',         // Circuit is open, calls fail fast
    HALF_OPEN = 'HALF_OPEN' // Testing if service has recovered
}

export class CircuitBreaker {
    private state: CircuitState = CircuitState.CLOSED;
    private failures = 0;
    private lastFailureTime = 0;
    private successCount = 0;
    private readonly options: CircuitBreakerOptions;

    constructor(options: Partial<CircuitBreakerOptions> = {}) {
        this.options = {
            failureThreshold: options.failureThreshold || 5,
            resetTimeout: options.resetTimeout || 60000, // 1 minute
            monitoringPeriod: options.monitoringPeriod || 300000, // 5 minutes
        };
    }

    /**
     * Execute an operation through the circuit breaker
     */
    async execute<T>(operation: () => Promise<T>, operationName: string = 'unknown'): Promise<T> {
        if (this.state === CircuitState.OPEN) {
            if (this.shouldAttemptReset()) {
                this.state = CircuitState.HALF_OPEN;
                console.log(`[CircuitBreaker] Circuit HALF_OPEN for ${operationName}, attempting reset`);
            } else {
                throw new Error(`Circuit breaker is OPEN for ${operationName}. Service temporarily unavailable.`);
            }
        }

        try {
            const result = await operation();
            this.onSuccess(operationName);
            return result;
        } catch (error) {
            this.onFailure(operationName);
            throw error;
        }
    }

    private onSuccess(operationName: string): void {
        this.failures = 0;
        this.successCount++;

        if (this.state === CircuitState.HALF_OPEN) {
            this.state = CircuitState.CLOSED;
            console.log(`[CircuitBreaker] Circuit CLOSED for ${operationName} - service recovered`);
        }
    }

    private onFailure(operationName: string): void {
        this.failures++;
        this.lastFailureTime = Date.now();

        if (this.failures >= this.options.failureThreshold) {
            this.state = CircuitState.OPEN;
            console.log(`[CircuitBreaker] Circuit OPEN for ${operationName} - ${this.failures} failures detected`);
        }
    }

    private shouldAttemptReset(): boolean {
        return Date.now() - this.lastFailureTime >= this.options.resetTimeout;
    }

    /**
     * Get current circuit state
     */
    getState(): CircuitState {
        return this.state;
    }

    /**
     * Get circuit statistics
     */
    getStats(): {
        state: CircuitState;
        failures: number;
        successCount: number;
        lastFailureTime: number;
    } {
        return {
            state: this.state,
            failures: this.failures,
            successCount: this.successCount,
            lastFailureTime: this.lastFailureTime,
        };
    }

    /**
     * Reset the circuit breaker to closed state
     */
    reset(): void {
        this.state = CircuitState.CLOSED;
        this.failures = 0;
        this.successCount = 0;
        this.lastFailureTime = 0;
        console.log('[CircuitBreaker] Circuit manually reset to CLOSED');
    }
}

/**
 * Global circuit breaker instances for different services
 */
class CircuitBreakerRegistry {
    private breakers = new Map<string, CircuitBreaker>();

    getOrCreate(name: string, options?: Partial<CircuitBreakerOptions>): CircuitBreaker {
        if (!this.breakers.has(name)) {
            this.breakers.set(name, new CircuitBreaker(options));
        }
        return this.breakers.get(name)!;
    }

    getAllStats(): Record<string, any> {
        const stats: Record<string, any> = {};
        for (const [name, breaker] of this.breakers) {
            stats[name] = breaker.getStats();
        }
        return stats;
    }

    resetAll(): void {
        for (const breaker of this.breakers.values()) {
            breaker.reset();
        }
    }
}

export const circuitBreakerRegistry = new CircuitBreakerRegistry();

/**
 * Higher-order function to wrap any async function with circuit breaker
 */
export function withCircuitBreaker<T extends any[], R>(
    breakerName: string,
    options?: Partial<CircuitBreakerOptions>
) {
    return (fn: (...args: T) => Promise<R>) => {
        return async (...args: T): Promise<R> => {
            const breaker = circuitBreakerRegistry.getOrCreate(breakerName, options);
            return breaker.execute(() => fn(...args), breakerName);
        };
    };
}