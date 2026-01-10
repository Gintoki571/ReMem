// src/integration/tools/DynamicSchemaToolRegistry.ts

import {
    SchemaLoader,
    createSchemaNode,
    handleSchemaUpdate,
    handleSchemaDelete
} from '@core/index.js';
import { formatToolResponse, formatToolError } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import type { SchemaBuilder } from '@core/index.js';

/**
 * Interface defining the public contract for dynamic schema tool registry
 */
export interface IDynamicSchemaToolRegistry {
    getTools(): Tool[];
    handleToolCall(toolName: string, args: Record<string, any>, knowledgeGraphManager: ApplicationManager): Promise<ToolResponse>;
    refresh(): Promise<void>;
}

/**
 * Manages dynamic tools generated from schema definitions across core and modules.
 */
class DynamicSchemaToolRegistry implements IDynamicSchemaToolRegistry {
    private schemas: Map<string, SchemaBuilder>;
    private toolsCache: Map<string, Tool>;
    private static instance: DynamicSchemaToolRegistry;

    private constructor() {
        this.schemas = new Map();
        this.toolsCache = new Map();
    }

    public static getInstance(): DynamicSchemaToolRegistry {
        if (!DynamicSchemaToolRegistry.instance) {
            DynamicSchemaToolRegistry.instance = new DynamicSchemaToolRegistry();
        }
        return DynamicSchemaToolRegistry.instance;
    }

    /**
     * Initializes the registry by loading modular schemas
     */
    public async initialize(): Promise<void> {
        return this.refresh();
    }

    /**
     * Reloads all schemas from core and active modules
     */
    public async refresh(): Promise<void> {
        try {
            this.schemas.clear();
            this.toolsCache.clear();

            // Load all modular schemas
            const loadedSchemas = await SchemaLoader.loadAllSchemas();
            for (const [name, builder] of Object.entries(loadedSchemas)) {
                this.schemas.set(name, builder);
            }

            // Generate tools for each schema
            for (const [schemaName, schema] of this.schemas.entries()) {
                const tools = await this.generateToolsForSchema(schemaName, schema);
                tools.forEach(tool => this.toolsCache.set(tool.name, tool));
            }

            console.error(`[ModularTools] Initialized ${this.schemas.size} schemas and ${this.toolsCache.size} tools`);
        } catch (error) {
            console.error('[ModularTools] Initialization error:', error);
            throw error;
        }
    }

    public getTools(): Tool[] {
        return Array.from(this.toolsCache.values());
    }

    private async generateToolsForSchema(schemaName: string, schema: SchemaBuilder): Promise<Tool[]> {
        const tools: Tool[] = [];
        const baseSchema = schema.build();

        // Add tool
        tools.push(baseSchema as unknown as Tool);

        // Update tool
        const updateSchema = schema.createUpdateSchema();
        tools.push(updateSchema as unknown as Tool);

        // Delete tool
        const deleteSchema: Tool = {
            name: `delete_${schemaName}`,
            description: `Delete an existing ${schemaName} from the knowledge graph`,
            inputSchema: {
                type: "object",
                properties: {
                    [`delete_${schemaName}`]: {
                        type: "object",
                        description: `Delete parameters for ${schemaName}`,
                        properties: {
                            name: {
                                type: "string",
                                description: `The name of the ${schemaName} to delete`
                            }
                        },
                        required: ["name"]
                    }
                },
                required: [`delete_${schemaName}`]
            }
        };

        tools.push(deleteSchema);
        return tools;
    }

    public async handleToolCall(
        toolName: string,
        args: Record<string, any>,
        knowledgeGraphManager: ApplicationManager
    ): Promise<ToolResponse> {
        const match = toolName.match(/^(add|update|delete)_(.+)$/);
        if (!match) {
            return formatToolError({
                operation: toolName,
                error: `Invalid tool name format: ${toolName}`,
                suggestions: ["Tool name must follow pattern: 'add|update|delete_<schemaName>'"]
            });
        }

        const [, operation, schemaName] = match;
        const schemaBuilder = this.schemas.get(schemaName);

        if (!schemaBuilder) {
            return formatToolError({
                operation: toolName,
                error: `Schema not found: ${schemaName}. Module might not be active.`,
                context: { availableTools: Array.from(this.toolsCache.keys()) },
                suggestions: ["Activate the appropriate module to use this tool"]
            });
        }

        try {
            const schema = schemaBuilder.build();

            switch (operation) {
                case 'add': {
                    const nodeData = args[schemaName];
                    const existingNodes = await knowledgeGraphManager.openNodes([nodeData.name]);

                    if (existingNodes.nodes.length > 0) {
                        throw new Error(`Node already exists: ${nodeData.name}`);
                    }

                    const { nodes, edges } = await createSchemaNode(nodeData, schema, schemaName);

                    await knowledgeGraphManager.beginTransaction();
                    try {
                        await knowledgeGraphManager.addNodes(nodes);
                        if (edges.length > 0) {
                            await knowledgeGraphManager.addEdges(edges);
                        }
                        await knowledgeGraphManager.commit();

                        return formatToolResponse({
                            data: { nodes, edges },
                            actionTaken: `Created ${schemaName}: ${nodeData.name}`
                        });
                    } catch (error) {
                        await knowledgeGraphManager.rollback();
                        throw error;
                    }
                }

                case 'update': {
                    return handleSchemaUpdate(
                        args[`update_${schemaName}`],
                        schema,
                        schemaName,
                        knowledgeGraphManager
                    );
                }

                case 'delete': {
                    const params = args[`delete_${schemaName}`];
                    const name = params?.name;
                    if (!name) {
                        return formatToolError({
                            operation: toolName,
                            error: `Name is required to delete a ${schemaName}`,
                            suggestions: ["Provide the 'name' parameter"]
                        });
                    }
                    return handleSchemaDelete(name, schemaName, knowledgeGraphManager);
                }

                default:
                    return formatToolError({
                        operation: toolName,
                        error: `Unknown operation: ${operation}`,
                        suggestions: ["Use 'add', 'update', or 'delete'"]
                    });
            }
        } catch (error) {
            return formatToolError({
                operation: toolName,
                error: error instanceof Error ? error.message : 'Unknown error occurred',
                context: { args },
                suggestions: [
                    "Check input parameters against schema",
                    "Verify entity existence for updates/deletes"
                ],
                recoverySteps: [
                    "Review schema requirements",
                    "Ensure all required fields are provided"
                ]
            });
        }
    }
}

export const dynamicSchemaTools = DynamicSchemaToolRegistry.getInstance();

export async function initializeDynamicTools(): Promise<IDynamicSchemaToolRegistry> {
    await dynamicSchemaTools.initialize();
    return dynamicSchemaTools;
}