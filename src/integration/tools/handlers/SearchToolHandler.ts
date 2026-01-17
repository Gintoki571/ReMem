import { BaseToolHandler } from './BaseToolHandler.js';
import { formatToolResponse, formatToolError, formatGraphAsNarrative } from '@shared/index.js';
import type { ToolResponse } from '@shared/index.js';
import { z } from 'zod';
import { CONFIG } from '@config/config.js';

// Input Schemas
const ReadGraphSchema = z.object({
    limit: z.number().optional(),
    offset: z.number().optional(),
});

const SearchNodesSchema = z.object({
    query: z.string().min(1).max(CONFIG.VALIDATION.MAX_TEXT_LENGTH),
    depth: z.number().min(1).max(CONFIG.SEARCH.MAX_DEPTH).optional().default(1),
});

const OpenNodesSchema = z.object({
    names: z.array(z.string().max(CONFIG.VALIDATION.MAX_NODE_NAME_LENGTH)).max(50), // Max 50 nodes
    depth: z.number().min(1).max(CONFIG.SEARCH.MAX_DEPTH).optional().default(1),
});

export class SearchToolHandler extends BaseToolHandler {
    async handleTool(name: string, args: Record<string, any>): Promise<ToolResponse> {
        try {
            switch (name) {
                case "read_graph": {
                    const params = this.validateSchema(ReadGraphSchema, args);
                    const graph = await this.knowledgeGraphManager.readGraph(params.limit, params.offset);
                    return formatToolResponse({
                        data: graph,
                        actionTaken: `Read knowledge graph (limit: ${params.limit || 'all'}, offset: ${params.offset || 0})`
                    });
                }

                case "search_nodes": {
                    const params = this.validateSchema(SearchNodesSchema, args);
                    const searchResults = await this.knowledgeGraphManager.searchNodes(params.query, params.depth);
                    return formatToolResponse({
                        data: searchResults,
                        actionTaken: `Searched nodes with query: ${params.query} (depth: ${params.depth})`,
                        message: formatGraphAsNarrative(searchResults)
                    });
                }

                case "open_nodes": {
                    const params = this.validateSchema(OpenNodesSchema, args);
                    const nodes = await this.knowledgeGraphManager.openNodes(params.names, params.depth);
                    return formatToolResponse({
                        data: nodes,
                        actionTaken: `Retrieved nodes: ${params.names.join(', ')} (depth: ${params.depth})`,
                        message: formatGraphAsNarrative(nodes)
                    });
                }

                default:
                    throw new Error(`Unknown search operation: ${name}`);
            }
        } catch (error) {
            return formatToolError({
                operation: name,
                error: error instanceof Error ? error.message : 'Unknown error occurred',
                context: { args },
                suggestions: [
                    "Check node names exist",
                    "Verify search query format",
                    "Ensure numeric limits are valid"
                ],
                recoverySteps: [
                    "Try with different node names",
                    "Adjust search query parameters"
                ]
            });
        }
    }
}