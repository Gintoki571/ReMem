// src/integration/tools/handlers/HealthToolHandler.ts

import { BaseToolHandler } from './BaseToolHandler.js';
import type { ToolResponse } from '@shared/index.js';
import { formatToolResponse, formatToolError } from '@shared/index.js';
import { getDatabase } from '@infrastructure/database/index.js';
import { getAllVectors, initVectorStore } from '@infrastructure/vector/VectorManager.js';
import { Logger } from '@core/logging/Logger.js';
import { CONFIG } from '@config/config.js';
import { ENV } from '@config/env.js';

export interface HealthStatus {
    status: 'healthy' | 'degraded' | 'unhealthy';
    timestamp: string;
    version: string;
    components: {
        database: ComponentHealth;
        vectorStore: ComponentHealth;
        memory: MemoryHealth;
    };
    configuration: {
        memoryStrategy: string;
        llmModel: string;
        hasApiKey: boolean;
        hasMcpAuth: boolean;
    };
}

interface ComponentHealth {
    status: 'up' | 'down' | 'unknown';
    latencyMs?: number;
    details?: string;
}

interface MemoryHealth {
    heapUsedMB: number;
    heapTotalMB: number;
    externalMB: number;
    rssMS: number;
}

export class HealthToolHandler extends BaseToolHandler {
    async handleTool(name: string, args: Record<string, unknown>): Promise<ToolResponse> {
        try {
            switch (name) {
                case 'health_check':
                    return await this.healthCheck();
                default:
                    return formatToolError({
                        operation: name,
                        error: `Unknown health tool: ${name}`,
                        suggestions: ['Use health_check']
                    });
            }
        } catch (error) {
            return this.handleError(name, error);
        }
    }

    private async healthCheck(): Promise<ToolResponse> {
        const startTime = Date.now();

        // Check database health
        const dbHealth = await this.checkDatabase();

        // Check vector store health
        const vectorHealth = await this.checkVectorStore();

        // Get memory stats
        const memoryHealth = this.getMemoryStats();

        // Determine overall status
        const overallStatus = this.determineOverallStatus(dbHealth, vectorHealth);

        const health: HealthStatus = {
            status: overallStatus,
            timestamp: new Date().toISOString(),
            version: CONFIG.SERVER.VERSION,
            components: {
                database: dbHealth,
                vectorStore: vectorHealth,
                memory: memoryHealth
            },
            configuration: {
                memoryStrategy: ENV.MEMORY_STRATEGY,
                llmModel: ENV.LLM_MODEL,
                hasApiKey: !!ENV.OPENAI_API_KEY,
                hasMcpAuth: !!ENV.MCP_API_KEY
            }
        };

        const totalLatency = Date.now() - startTime;
        Logger.info('Health', `Health check completed in ${totalLatency}ms: ${overallStatus}`);

        return formatToolResponse({
            message: `System is ${overallStatus}`,
            data: health
        });
    }

    private async checkDatabase(): Promise<ComponentHealth> {
        const start = Date.now();
        try {
            const db = getDatabase();
            // Simple query to verify connectivity
            const result = await db.query.nodes.findFirst();
            const latency = Date.now() - start;
            return {
                status: 'up',
                latencyMs: latency,
                details: result ? 'Has data' : 'Empty'
            };
        } catch (error) {
            Logger.error('Health', `Database check failed: ${error}`);
            return {
                status: 'down',
                latencyMs: Date.now() - start,
                details: error instanceof Error ? error.message : 'Unknown error'
            };
        }
    }

    private async checkVectorStore(): Promise<ComponentHealth> {
        const start = Date.now();
        try {
            await initVectorStore();
            const vectors = await getAllVectors();
            const latency = Date.now() - start;
            return {
                status: 'up',
                latencyMs: latency,
                details: `${vectors.length} vectors`
            };
        } catch (error) {
            Logger.error('Health', `Vector store check failed: ${error}`);
            return {
                status: 'down',
                latencyMs: Date.now() - start,
                details: error instanceof Error ? error.message : 'Unknown error'
            };
        }
    }

    private getMemoryStats(): MemoryHealth {
        const mem = process.memoryUsage();
        return {
            heapUsedMB: Math.round(mem.heapUsed / 1024 / 1024),
            heapTotalMB: Math.round(mem.heapTotal / 1024 / 1024),
            externalMB: Math.round(mem.external / 1024 / 1024),
            rssMS: Math.round(mem.rss / 1024 / 1024)
        };
    }

    private determineOverallStatus(
        db: ComponentHealth,
        vector: ComponentHealth
    ): 'healthy' | 'degraded' | 'unhealthy' {
        if (db.status === 'down') return 'unhealthy';
        if (vector.status === 'down') return 'degraded';
        return 'healthy';
    }
}
