// src/tools/handlers/AutoMemoryToolHandler.ts

import { BaseToolHandler } from './BaseToolHandler.js';
import { handleAutoAddMemory, handleSemanticSearch } from './autoMemoryHandler.js';
import type { ToolResponse } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';

/**
 * Handler for auto memory tools (auto_add_memory, semantic_search)
 */
export class AutoMemoryToolHandler extends BaseToolHandler {
    constructor(manager: ApplicationManager) {
        super(manager);
    }

    async handleTool(name: string, args: Record<string, any>): Promise<ToolResponse> {
        switch (name) {
            case 'auto_add_memory':
                return handleAutoAddMemory(
                    args as { text: string; generateEmbeddings?: boolean },
                    this.knowledgeGraphManager
                );
            case 'semantic_search':
                return handleSemanticSearch(
                    args as { query: string; limit?: number },
                    this.knowledgeGraphManager
                );
            default:
                throw new Error(`Unknown auto memory tool: ${name}`);
        }
    }
}
