#!/usr/bin/env node
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
    CallToolRequestSchema,
    ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { ApplicationManager } from '@application/managers/ApplicationManager.js';
import { toolsRegistry } from '@integration/tools/registry/toolsRegistry.js';
import { CONFIG } from './config/config.js';

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
            const coreTools = [
                'auto_add_memory',
                'semantic_search',
                'hybrid_search',
                'search_nodes',
                'open_nodes',
                'delete_nodes',
                'read_graph',
                'query_sql_db',
                'list_modules',
                'activate_module',
                'deactivate_module'
            ];

            const allTools = toolsRegistry.getAllTools();
            // Show all Core tools + any tools from ACTIVE modules
            const visibleTools = allTools.filter(tool => {
                const isCore = coreTools.includes(tool.name);
                // Dynamic tools follow the pattern add_|update_|delete_
                // We show them if they are in the registry (Registry already filters by active modules)
                return isCore || tool.name.includes('_');
            });

            return {
                tools: visibleTools.map(tool => ({
                    name: tool.name,
                    description: tool.description,
                    inputSchema: tool.inputSchema
                }))
            };
        });

        server.setRequestHandler(CallToolRequestSchema, async (request) => {
            const { name, arguments: args } = request.params;
            const result = await toolsRegistry.handleToolCall(name, args ?? {});
            return {
                toolResult: result.toolResult
            };
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