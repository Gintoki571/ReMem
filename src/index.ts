#!/usr/bin/env node
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
    CallToolRequestSchema,
    ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { ApplicationManager } from '@application/managers/ApplicationManager.js';
import { handleCallToolRequest } from '@integration/tools/callToolHandler.js';
import { toolsRegistry } from '@integration/tools/registry/toolsRegistry.js';
import { CONFIG } from './config/config.js';
import { formatToolError } from "@shared/utils/responseFormatter.js";

const knowledgeGraphManager = new ApplicationManager();

const server = new Server({
    name: CONFIG.SERVER.NAME,
    version: CONFIG.SERVER.VERSION,
}, {
    capabilities: {
        tools: {},
    },
});

async function main(): Promise<void> {
    try {
        await toolsRegistry.initialize(knowledgeGraphManager);

        server.setRequestHandler(ListToolsRequestSchema, async () => {
            // [MINIMALIST MODE - OPTION B]
            // We only expose a small set of "Smart" tools to the AI to save context space.
            // The "Manual" tools still exist in the code but are hidden from the AI's view.
            const essentialTools = [
                'auto_add_memory',
                'semantic_search',
                'hybrid_search',
                'search_nodes',
                'open_nodes',
                'delete_nodes',
                'read_graph',
                'query_sql_db'
            ];

            const allTools = toolsRegistry.getAllTools();
            const visibleTools = allTools.filter(tool => essentialTools.includes(tool.name));

            return {
                tools: visibleTools.map(tool => ({
                    name: tool.name,
                    description: tool.description,
                    inputSchema: tool.inputSchema
                }))
            };
        });

        server.setRequestHandler(CallToolRequestSchema, async (request) => {
            try {
                if (!request.params.arguments) {
                    throw new Error("Tool arguments are required");
                }

                const toolRequest = {
                    params: {
                        name: request.params.name,
                        arguments: request.params.arguments
                    }
                };

                const result = await handleCallToolRequest(toolRequest, knowledgeGraphManager);

                return {
                    toolResult: result.toolResult
                };
            } catch (error) {
                console.error("Error in handleCallToolRequest:", error);
                const formattedError = formatToolError({
                    operation: "callTool",
                    error: error instanceof Error ? error.message : 'Unknown error occurred',
                    context: { request },
                    suggestions: ["Examine the tool input parameters for correctness.", "Verify that the requested operation is supported."],
                    recoverySteps: ["Adjust the input parameters based on the schema definition."]
                });
                return {
                    toolResult: formattedError.toolResult
                };
            }
        });

        server.onerror = (error: Error) => {
            console.error("[MCP Server Error]", error);
        };

        process.on('SIGINT', async () => {
            await server.close();
            process.exit(0);
        });

        const transport = new StdioServerTransport();
        await server.connect(transport);
        console.error("Knowledge Graph MCP Server running on stdio");
    } catch (error) {
        console.error("Fatal error during server startup:", error);
        process.exit(1);
    }
}

main().catch((error) => {
    console.error("Fatal error in main():", error);
    process.exit(1);
});