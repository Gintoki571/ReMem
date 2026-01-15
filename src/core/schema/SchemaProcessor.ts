// src/schema/loader/schemaProcessor.ts

import { formatToolResponse, formatToolError, ToolResponse } from '@shared/index.js';
import type { Node, Edge, Graph, SchemaConfig } from '@core/index.js';
import type { ApplicationManager } from '@application/index.js';

interface NodeData {
    name: string;

    [key: string]: any;
}

export interface ProcessedNodeResult {
    nodes: Node[];
    edges: Edge[];
}

export interface SchemaUpdateResult {
    metadata: Record<string, unknown>;
    edgeChanges: {
        remove: Edge[];
        add: Edge[];
    };
}

/**
 * Formats a field value into a metadata string.
 */
function formatMetadataEntry(field: string, value: string | string[] | unknown): string {
    if (Array.isArray(value)) {
        return `${field}: ${value.join(', ')}`;
    }
    return `${field}: ${String(value)}`;
}

/**
 * Creates a node based on schema definition and input data.
 */
export async function createSchemaNode(
    data: NodeData,
    schema: SchemaConfig,
    nodeType: string
): Promise<ProcessedNodeResult> {
    try {
        const { metadataConfig, relationships } = schema;
        const metadata: Record<string, unknown> = {};
        const nodes: Node[] = [];
        const edges: Edge[] = [];

        // Create excluded fields set
        const excludedFields = new Set<string>([
            'name',
            ...metadataConfig.requiredFields,
            ...metadataConfig.optionalFields,
            ...(metadataConfig.excludeFields || []),
        ]);

        if (relationships) {
            Object.keys(relationships).forEach(field => excludedFields.add(field));
        }

        // Process required fields
        for (const field of metadataConfig.requiredFields) {
            if (data[field] === undefined) {
                throw new Error(`Required field "${field}" is missing`);
            }
            if (!relationships || !relationships[field]) {
                metadata[field] = data[field];
            }
        }

        // Process optional fields
        for (const field of metadataConfig.optionalFields) {
            if (data[field] !== undefined && (!relationships || !relationships[field])) {
                metadata[field] = data[field];
            }
        }

        // Process relationships
        if (relationships) {
            for (const [field, config] of Object.entries(relationships)) {
                if (data[field]) {
                    const value = data[field];
                    if (Array.isArray(value)) {
                        for (const target of value) {
                            edges.push({
                                type: 'edge',
                                from: data.name,
                                to: target,
                                edgeType: config.edgeType
                            });
                        }
                    } else {
                        edges.push({
                            type: 'edge',
                            from: data.name,
                            to: value as string,
                            edgeType: config.edgeType
                        });
                    }
                    metadata[field] = value;
                }
            }
        }

        // Process additional fields
        for (const [key, value] of Object.entries(data)) {
            if (!excludedFields.has(key) && value !== undefined) {
                metadata[key] = value;
            }
        }

        // Create the main node
        const node: Node = {
            type: 'node',
            name: data.name,
            nodeType,
            metadata: metadata as Record<string, unknown>
        };
        nodes.push(node);

        return { nodes, edges };
    } catch (error) {
        throw error;
    }
}

export async function updateSchemaNode(
    updates: NodeData,
    currentNode: Node,
    schema: SchemaConfig,
    currentGraph: Graph
): Promise<SchemaUpdateResult> {
    const { metadataConfig, relationships } = schema;
    // Initialize metadata with current state
    const metadata: Record<string, unknown> = { ...currentNode.metadata } as Record<string, unknown>;
    const edgeChanges = {
        remove: [] as Edge[],
        add: [] as Edge[]
    };

    // Create a set of all schema-defined fields
    const schemaFields = new Set<string>([
        ...metadataConfig.requiredFields,
        ...metadataConfig.optionalFields,
        ...(metadataConfig.excludeFields || []),
        'name',
        'metadata'
    ]);

    // Add relationship fields to schema fields
    if (relationships) {
        Object.keys(relationships).forEach(field => schemaFields.add(field));
    }

    // Process standard metadata fields
    const allSchemaFields = [...metadataConfig.requiredFields, ...metadataConfig.optionalFields];
    for (const field of allSchemaFields) {
        if (updates[field] !== undefined && (!relationships || !relationships[field])) {
            metadata[field] = updates[field];
        }
    }

    // Process relationships if they exist in the schema
    if (relationships) {
        for (const [field, config] of Object.entries(relationships)) {
            // Only process relationship if it's being updated
            if (updates[field] !== undefined) {
                // Get all existing edges for this relationship type from this node
                const existingEdges = currentGraph.edges.filter(edge =>
                    edge.from === currentNode.name &&
                    edge.edgeType === config.edgeType
                );

                // Only mark edges for removal if they're part of this relationship type
                edgeChanges.remove.push(...existingEdges);

                // Add new edges
                const value = updates[field];
                if (Array.isArray(value)) {
                    value.forEach((target: string) => {
                        edgeChanges.add.push({
                            type: 'edge',
                            from: currentNode.name,
                            to: target,
                            edgeType: config.edgeType
                        });
                    });
                } else if (value) {
                    edgeChanges.add.push({
                        type: 'edge',
                        from: currentNode.name,
                        to: value as string,
                        edgeType: config.edgeType
                    });
                }

                metadata[field] = value;
            }
        }
    }

    // Process additional fields not defined in schema
    for (const [key, value] of Object.entries(updates)) {
        if (!schemaFields.has(key) && value !== undefined) {
            metadata[key] = value;
        }
    }

    return {
        metadata,
        edgeChanges
    };
}

/**
 * Handles the complete update process for a schema-based entity.
 */
export async function handleSchemaUpdate(
    updates: NodeData,
    schema: SchemaConfig,
    nodeType: string,
    applicationManager: ApplicationManager
): Promise<ToolResponse> {
    try {
        // Start a transaction to ensure atomic updates
        await applicationManager.beginTransaction();

        // Get the complete current state
        const fullGraph = await applicationManager.readGraph();
        const node = fullGraph.nodes.find((n: Node) => n.nodeType === nodeType && n.name === updates.name);

        if (!node) {
            await applicationManager.rollback();
            return formatToolError({
                operation: 'updateSchema',
                error: `${nodeType} "${updates.name}" not found`,
                context: { updates, nodeType },
                suggestions: ["Verify the node exists", "Check node type matches"]
            });
        }

        try {
            // Process updates
            const { metadata, edgeChanges } = await updateSchemaNode(
                updates,
                node,
                schema,
                fullGraph
            );

            // Update the node first
            const updatedNode: Node = {
                ...node,
                metadata
            };
            await applicationManager.updateNodes([updatedNode]);

            // Then handle edges if there are any changes
            if (edgeChanges.remove.length > 0) {
                await applicationManager.deleteEdges(edgeChanges.remove);
            }

            if (edgeChanges.add.length > 0) {
                await applicationManager.addEdges(edgeChanges.add);
            }

            // If everything succeeded, commit the transaction
            await applicationManager.commit();

            return formatToolResponse({
                data: {
                    updatedNode,
                    edgeChanges
                },
                actionTaken: `Updated ${nodeType}: ${updatedNode.name}`
            });

        } catch (error) {
            // If anything fails, rollback all changes
            await applicationManager.rollback();
            throw error;
        }

    } catch (error) {
        if (applicationManager.isInTransaction()) {
            await applicationManager.rollback();
        }

        return formatToolError({
            operation: 'updateSchema',
            error: error instanceof Error ? error.message : 'Unknown error occurred',
            context: { updates, schema, nodeType },
            suggestions: [
                "Check all required fields are provided",
                "Verify relationship targets exist"
            ],
            recoverySteps: [
                "Review schema requirements",
                "Ensure node exists before updating"
            ]
        });
    }
}

export async function handleSchemaDelete(
    nodeName: string,
    nodeType: string,
    applicationManager: ApplicationManager
): Promise<ToolResponse> {
    try {
        const graph = await applicationManager.readGraph();
        const node = graph.nodes.find((n: Node) => n.name === nodeName && n.nodeType === nodeType);

        if (!node) {
            return formatToolError({
                operation: 'deleteSchema',
                error: `${nodeType} "${nodeName}" not found`,
                context: { nodeName, nodeType },
                suggestions: ["Verify node name and type"]
            });
        }

        await applicationManager.deleteNodes([nodeName]);

        return formatToolResponse({
            actionTaken: `Deleted ${nodeType}: ${nodeName}`
        });
    } catch (error) {
        return formatToolError({
            operation: 'deleteSchema',
            error: error instanceof Error ? error.message : 'Unknown error occurred',
            context: { nodeName, nodeType },
            suggestions: [
                "Check node exists",
                "Verify delete permissions"
            ],
            recoverySteps: [
                "Ensure no dependent nodes exist",
                "Try retrieving node first"
            ]
        });
    }
}