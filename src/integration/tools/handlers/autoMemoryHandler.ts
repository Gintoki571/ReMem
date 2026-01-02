import { analyzer } from '@application/services/Analyzer.js';
import { addVector, searchVectors, VectorRecord } from '@infrastructure/vector/VectorManager.js';
import { getDatabase, schema } from '@infrastructure/database/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import type { Node, Edge } from '@core/index.js';

/**
 * auto_add_memory - Automatically extracts entities and relationships from text
 * and adds them to the knowledge graph
 */
export const autoAddMemoryTool: Tool = {
    name: 'auto_add_memory',
    description: 'Automatically analyze text to extract and store entities, relationships, and semantic embeddings. Use this when you want to remember something without manually specifying the structure.',
    inputSchema: {
        type: 'object',
        properties: {
            text: {
                type: 'string',
                description: 'The text to analyze and extract memory from',
            },
            generateEmbeddings: {
                type: 'boolean',
                description: 'Whether to generate vector embeddings for semantic search (requires API key). Defaults to true.',
            },
        },
        required: ['text'],
    },
};

/**
 * semantic_search - Search memories by meaning, not just exact match
 */
export const semanticSearchTool: Tool = {
    name: 'semantic_search',
    description: 'Search memories using semantic similarity. Returns memories related to the query meaning, not just keyword matches.',
    inputSchema: {
        type: 'object',
        properties: {
            query: {
                type: 'string',
                description: 'The search query - describe what you are looking for',
            },
            limit: {
                type: 'number',
                description: 'Maximum number of results to return. Defaults to 5.',
            },
        },
        required: ['query'],
    },
};

/**
 * Handle auto_add_memory tool call
 */
export async function handleAutoAddMemory(
    args: { text: string; generateEmbeddings?: boolean },
    manager: ApplicationManager
): Promise<ToolResponse> {
    try {
        const db = getDatabase();
        const addedNodes: string[] = [];
        const addedEdges: string[] = [];
        const errors: string[] = [];

        // Step 1: Extract entities using LLM
        // console.error('[AutoAdd] Starting extraction for text:', args.text.substring(0, 50) + '...');
        const extraction = await analyzer.extractFromText(args.text);
        // console.error('[AutoAdd] Extraction complete. Found entities:', extraction.entities.length);

        if (!extraction.entities.length && !extraction.relationships.length) {
            return {
                toolResult: {
                    isError: false,
                    data: null,
                    actionTaken: 'No entities or relationships found',
                    timestamp: new Date().toISOString(),
                    content: [{ type: 'text', text: 'No entities or relationships were found in the text.' }]
                }
            };
        }

        // Step 2: Add nodes to graph (JSON)
        // Note: Node is an interface/type, so we create a plain object
        const nodesToAdd: Node[] = extraction.entities.map(e => ({
            type: 'node',
            name: e.name,
            nodeType: e.nodeType,
            metadata: e.metadata,
        }));

        if (nodesToAdd.length > 0) {
            try {
                const addedNodeResults = await manager.addNodes(nodesToAdd);
                addedNodeResults.forEach(n => addedNodes.push(n.name));

                // Also add to SQLite for relational queries
                for (const entity of extraction.entities) {
                    try {
                        db.insert(schema.nodes).values({
                            name: entity.name,
                            nodeType: entity.nodeType,
                            metadata: JSON.stringify(entity.metadata),
                        }).onConflictDoNothing().run();
                    } catch (dbError) {
                        // console.error('[AutoAdd] SQLite error:', dbError);
                    }
                }

                // Step 3: Generate embeddings if requested
                if (args.generateEmbeddings !== false && process.env.OPENAI_API_KEY) {
                    for (const entity of extraction.entities) {
                        try {
                            const textForEmbedding = analyzer.summarizeForEmbedding(
                                entity.name,
                                entity.nodeType,
                                entity.metadata
                            );
                            const embedding = await analyzer.generateEmbedding(textForEmbedding);

                            const vectorRecord: VectorRecord = {
                                id: `${entity.name}-${Date.now()}`,
                                text: textForEmbedding,
                                vector: embedding,
                                nodeName: entity.name,
                                nodeType: entity.nodeType,
                                metadata: JSON.stringify(entity.metadata),
                            };

                            await addVector(vectorRecord);
                        } catch (embedError) {
                            console.error('[AutoAdd] Embedding error:', embedError);
                        }
                    }
                }
            } catch (nodeError) {
                errors.push(`Failed to add nodes: ${nodeError}`);
            }
        }

        // Step 4: Prepare edges in correct format
        const edgesToAdd: Edge[] = extraction.relationships.map(rel => ({
            type: 'edge' as const,
            from: rel.from,
            to: rel.to,
            edgeType: rel.edgeType,
        }));

        // Add edges
        if (edgesToAdd.length > 0) {
            try {
                const addedEdgeResults = await manager.addEdges(edgesToAdd);
                addedEdgeResults.forEach(e => addedEdges.push(`${e.from} -[${e.edgeType}]-> ${e.to}`));

                // Also add to SQLite
                for (const rel of extraction.relationships) {
                    try {
                        db.insert(schema.edges).values({
                            fromNode: rel.from,
                            toNode: rel.to,
                            edgeType: rel.edgeType,
                        }).onConflictDoNothing().run();
                    } catch (dbError) {
                        console.error('[AutoAdd] SQLite edge error:', dbError);
                    }
                }
            } catch (edgeError) {
                errors.push(`Failed to add edges: ${edgeError}`);
            }
        }

        return {
            toolResult: {
                isError: false,
                data: {
                    nodesAdded: addedNodes,
                    edgesAdded: addedEdges,
                    errors: errors.length > 0 ? errors : undefined,
                },
                actionTaken: `Extracted and added ${addedNodes.length} entities and ${addedEdges.length} relationships`,
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: `Successfully processed: ${addedNodes.length} entities, ${addedEdges.length} relationships${errors.length > 0 ? `. Errors: ${errors.length}` : ''}`,
                }],
            },
        };
    } catch (error) {
        return {
            toolResult: {
                isError: true,
                data: null,
                actionTaken: 'auto_add_memory failed',
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`,
                }],
            },
        };
    }
}

/**
 * Handle semantic_search tool call
 */
export async function handleSemanticSearch(
    args: { query: string; limit?: number },
    _manager: ApplicationManager
): Promise<ToolResponse> {
    try {
        if (!process.env.OPENAI_API_KEY) {
            return {
                toolResult: {
                    isError: true,
                    data: null,
                    actionTaken: 'semantic_search requires OPENAI_API_KEY',
                    timestamp: new Date().toISOString(),
                    content: [{
                        type: 'text',
                        text: 'Semantic search requires OPENAI_API_KEY to be set for generating query embeddings.',
                    }],
                },
            };
        }

        // Generate embedding for query
        const queryEmbedding = await analyzer.generateEmbedding(args.query);

        // Search in vector store
        const results = await searchVectors(queryEmbedding, args.limit || 5);

        if (results.length === 0) {
            return {
                toolResult: {
                    isError: false,
                    data: [],
                    actionTaken: 'No matching memories found',
                    timestamp: new Date().toISOString(),
                    content: [{
                        type: 'text',
                        text: 'No memories found matching the query.',
                    }],
                },
            };
        }

        const formattedResults = results.map((r, i) => ({
            rank: i + 1,
            nodeName: r.nodeName,
            nodeType: r.nodeType,
            text: r.text,
        }));

        return {
            toolResult: {
                isError: false,
                data: formattedResults,
                actionTaken: `Found ${results.length} matching memories`,
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: formattedResults.map(r => `${r.rank}. [${r.nodeType}] ${r.nodeName}: ${r.text}`).join('\n'),
                }],
            },
        };
    } catch (error) {
        return {
            toolResult: {
                isError: true,
                data: null,
                actionTaken: 'semantic_search failed',
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`,
                }],
            },
        };
    }
}

export const autoMemoryTools = [autoAddMemoryTool, semanticSearchTool];
