import { BaseToolHandler } from './BaseToolHandler.js';
import type { ToolResponse } from '@shared/index.js';
import { formatToolResponse } from '@shared/index.js';
import type { Node } from '@core/index.js';

export class GlobalMemoryHandler extends BaseToolHandler {
    async handleTool(toolName: string, args: Record<string, any>): Promise<ToolResponse> {
        switch (toolName) {
            case 'add_global_memory':
                return this.addGlobalMemory(args);
            default:
                throw new Error(`Unknown tool: ${toolName}`);
        }
    }

    private async addGlobalMemory(args: Record<string, any>): Promise<ToolResponse> {
        const { facts } = args;

        if (!Array.isArray(facts)) {
            throw new Error('facts must be an array of strings');
        }

        const nodesToAdd: Node[] = facts.map((fact: string, index: number) => ({
            type: 'node',
            name: `global_fact_${Date.now()}_${index}`,
            nodeType: 'global_fact',
            metadata: {
                content: fact,
                scope: 'global',
                timestamp: new Date().toISOString()
            }
        }));

        await this.knowledgeGraphManager.addNodes(nodesToAdd);

        return formatToolResponse({ message: `Successfully added ${facts.length} global facts to memory.` });
    }
}
