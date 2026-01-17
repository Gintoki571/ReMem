import { CONFIG } from '../../config/config.js';

interface ClientState {
    requestCount: number;
    windowStart: number;
    isBlocked: boolean;
    blockExpiresAt?: number;
}

/**
 * A memory-efficient Token Bucket / Sliding Window hybrid rate limiter.
 * Designed for stateless protocols like HTTP/JSON-RPC (stdio).
 * 
 * Since this is a local MCP server, we primarily identify clients by connection ID or
 * treat the entire stdio stream as one 'client' (singleton).
 * 
 * For multi-tenant scenarios, we would key by clientId/IP.
 */
export class RateLimiter {
    private clients = new Map<string, ClientState>();
    private cleanupInterval: NodeJS.Timeout;

    constructor() {
        // Cleanup expired clients every 5 minutes to prevent memory leaks
        this.cleanupInterval = setInterval(() => this.cleanup(), 5 * 60 * 1000);
    }

    /**
     * Check if a request is allowed for the given client ID.
     * For local stdio, use a constant ID like 'local-user'.
     */
    public isAllowed(clientId: string = 'global'): { allowed: boolean; error?: string } {
        const now = Date.now();
        let client = this.clients.get(clientId);

        // 1. Initialize client if new
        if (!client) {
            client = {
                requestCount: 0,
                windowStart: now,
                isBlocked: false
            };
            this.clients.set(clientId, client);
        }

        // 2. Check Block Status
        if (client.isBlocked) {
            if (client.blockExpiresAt && now > client.blockExpiresAt) {
                // Block expired, reset
                client.isBlocked = false;
                client.blockExpiresAt = undefined;
                client.requestCount = 0;
                client.windowStart = now;
            } else {
                const remainingSeconds = Math.ceil((client.blockExpiresAt! - now) / 1000);
                return {
                    allowed: false,
                    error: `Too many requests. You are blocked for ${remainingSeconds} seconds.`
                };
            }
        }

        // 3. Sliding Window Check
        if (now - client.windowStart > CONFIG.RATE_LIMIT.WINDOW_MS) {
            // New window
            client.windowStart = now;
            client.requestCount = 0;
        }

        // 4. Increment and Verify
        client.requestCount++;

        if (client.requestCount > CONFIG.RATE_LIMIT.MAX_REQUESTS) {
            client.isBlocked = true;
            client.blockExpiresAt = now + CONFIG.RATE_LIMIT.BLOCK_DURATION_MS;
            return {
                allowed: false,
                error: `Rate limit exceeded. Blocked for ${CONFIG.RATE_LIMIT.BLOCK_DURATION_MS / 1000 / 60} minutes.`
            };
        }

        return { allowed: true };
    }

    /**
     * Manually unblock a client (admin tool).
     */
    public reset(clientId: string = 'global'): void {
        this.clients.delete(clientId);
    }

    /**
     * Stop the cleanup interval.
     */
    public dispose(): void {
        clearInterval(this.cleanupInterval);
    }

    private cleanup(): void {
        const now = Date.now();
        for (const [id, client] of this.clients.entries()) {
            const isWindowExpired = now - client.windowStart > CONFIG.RATE_LIMIT.WINDOW_MS;
            const isBlockExpired = !client.isBlocked || (client.blockExpiresAt && now > client.blockExpiresAt);

            if (isWindowExpired && isBlockExpired) {
                this.clients.delete(id);
            }
        }
    }
}