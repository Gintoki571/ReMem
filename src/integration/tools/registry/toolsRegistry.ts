// src/integration/tools/registry/toolsRegistry.ts

import { allStaticTools } from './staticTools.js';
import { dynamicToolManager } from './dynamicTools.js';
import { formatToolError } from '@shared/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import { ToolHandlerFactory } from '../handlers/ToolHandlerFactory.js';
import type { ApplicationManager } from '@application/index.js';

/**
 * Central registry for all tools (both static and dynamic)
 */
export class ToolsRegistry {
    private static instance: ToolsRegistry;
    private initialized = false;
    private tools: Map<string, Tool> = new Map();
    private knowledgeGraphManager: ApplicationManager | null = null;

    private constructor() { }

    static getInstance(): ToolsRegistry {
        if (!ToolsRegistry.instance) {
            ToolsRegistry.instance = new ToolsRegistry();
        }
        return ToolsRegistry.instance;
    }

    async initialize(knowledgeGraphManager: ApplicationManager): Promise<void> {
        if (this.initialized) return;

        try {
            this.knowledgeGraphManager = knowledgeGraphManager;
            await this.refresh();
            this.initialized = true;
        } catch (error) {
            console.error('[ToolsRegistry] Initialization error:', error);
            throw error;
        }
    }

    /**
     * Refreshes the tools list, especially after module changes
     */
    async refresh(): Promise<void> {
        this.tools.clear();

        // 1. Register static tools
        allStaticTools.forEach(tool => {
            this.tools.set(tool.name, tool);
        });

        // 2. Initialize and register dynamic tools (Modular)
        await dynamicToolManager.initialize();
        dynamicToolManager.getTools().forEach(tool => {
            this.tools.set(tool.name, tool);
        });

        console.error(`[ToolsRegistry] Refreshed: ${this.tools.size} tools active.`);
    }

    getTool(name: string): Tool | undefined {
        return this.tools.get(name);
    }

    getAllTools(): Tool[] {
        return Array.from(this.tools.values());
    }

    async handleToolCall(toolName: string, args: Record<string, any>): Promise<ToolResponse> {
        if (!this.initialized || !this.knowledgeGraphManager) {
            return formatToolError({
                operation: toolName,
                error: 'ToolsRegistry not fully initialized',
                suggestions: ["Wait for initialization"]
            });
        }

        try {
            if (!this.tools.has(toolName)) {
                return formatToolError({
                    operation: toolName,
                    error: `Tool not found: ${toolName}`,
                    context: { activeTools: Array.from(this.tools.keys()) },
                    suggestions: ["Check if the module for this tool is active"]
                });
            }

            if (!ToolHandlerFactory.isInitialized()) {
                ToolHandlerFactory.initialize(this.knowledgeGraphManager);
            }

            const handler = ToolHandlerFactory.getHandler(toolName);
            return await handler.handleTool(toolName, args);
        } catch (error) {
            return formatToolError({
                operation: toolName,
                error: error instanceof Error ? error.message : 'Unknown error',
                context: { toolName, args }
            });
        }
    }

    hasTool(name: string): boolean {
        return this.tools.has(name);
    }
}

export const toolsRegistry = ToolsRegistry.getInstance();