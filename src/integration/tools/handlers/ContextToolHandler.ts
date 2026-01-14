import { BaseToolHandler } from './BaseToolHandler.js';
import { handleLogInteraction, handleGetContext } from './contextTools.js';
import type { ToolResponse } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';

/**
 * Handler for context management tools (log_interaction, get_context)
 */
export class ContextToolHandler extends BaseToolHandler {
    constructor(manager: ApplicationManager) {
        super(manager);
    }

    async handleTool(name: string, args: Record<string, any>): Promise<ToolResponse> {
        switch (name) {
            case 'log_interaction':
                return handleLogInteraction(
                    args as { role: 'user' | 'assistant' | 'system'; content: string },
                    this.knowledgeGraphManager
                );
            case 'get_context':
                return handleGetContext(
                    args,
                    this.knowledgeGraphManager
                );
            default:
                throw new Error(`Unknown context tool: ${name}`);
        }
    }
}
