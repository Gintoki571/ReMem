// src/tools/handlers/ToolHandlerFactory.ts

import { GraphToolHandler } from './GraphToolHandler.js';
import { SearchToolHandler } from './SearchToolHandler.js';
import { MetadataToolHandler } from './MetadataToolHandler.js';
import { DynamicToolHandler } from './DynamicToolHandler.js';
import { AutoMemoryToolHandler } from './AutoMemoryToolHandler.js';
import { SqlToolHandler } from './SqlToolHandler.js';
import { toolsRegistry } from '@integration/index.js';
import type { ApplicationManager } from '@application/index.js';
import type { BaseToolHandler } from './BaseToolHandler.js';

export class ToolHandlerFactory {
    private static graphHandler: GraphToolHandler;
    private static searchHandler: SearchToolHandler;
    private static metadataHandler: MetadataToolHandler;
    private static dynamicHandler: DynamicToolHandler;
    private static autoMemoryHandler: AutoMemoryToolHandler;
    private static sqlHandler: SqlToolHandler;
    private static initialized = false;

    /**
     * Initializes all tool handlers
     */
    static initialize(knowledgeGraphManager: ApplicationManager): void {
        if (this.initialized) {
            return;
        }

        this.graphHandler = new GraphToolHandler(knowledgeGraphManager);
        this.searchHandler = new SearchToolHandler(knowledgeGraphManager);
        this.metadataHandler = new MetadataToolHandler(knowledgeGraphManager);
        this.dynamicHandler = new DynamicToolHandler(knowledgeGraphManager);
        this.autoMemoryHandler = new AutoMemoryToolHandler(knowledgeGraphManager);
        this.sqlHandler = new SqlToolHandler(knowledgeGraphManager);

        this.initialized = true;
    }

    /**
     * Gets the appropriate handler for a given tool name
     */
    static getHandler(toolName: string): BaseToolHandler {
        if (!this.initialized) {
            throw new Error('ToolHandlerFactory not initialized');
        }

        // Check auto memory tools first
        if (toolName === 'auto_add_memory' || toolName === 'semantic_search' || toolName === 'hybrid_search') {
            return this.autoMemoryHandler;
        }

        // Then check static tools
        if (toolName.match(/^(add|update|delete)_(nodes|edges)$/)) {
            return this.graphHandler;
        }
        if (toolName.match(/^(read_graph|search_nodes|open_nodes)$/)) {
            return this.searchHandler;
        }
        if (toolName.match(/^(add|delete)_metadata$/)) {
            return this.metadataHandler;
        }
        if (toolName === 'query_sql_db') {
            return this.sqlHandler;
        }

        // Then check dynamic tools
        if (toolsRegistry.hasTool(toolName) && toolName.match(/^(add|update|delete)_/)) {
            return this.dynamicHandler;
        }

        throw new Error(`No handler found for tool: ${toolName}`);
    }

    /**
     * Checks if factory is initialized
     */
    static isInitialized(): boolean {
        return this.initialized;
    }
}