import { analyzer } from '@application/services/Analyzer.js';
import { addVector, searchVectors, VectorRecord } from '@infrastructure/vector/VectorManager.js';
import { getDatabase, schema } from '@infrastructure/database/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import { formatGraphAsNarrative } from '@shared/index.js';
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
 * hybrid_search - Search memories using both keywords and semantic similarity
 */
export const hybridSearchTool: Tool = {
    name: 'hybrid_search',
    description: 'Advanced search that combines semantic meaning with keyword matching for high accuracy results. It uses Reciprocal Rank Fusion (RRF) to score results.',
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
            depth: {
                type: 'number',
                description: 'BFS depth for exploring connected memories. Defaults to 1.',
            }
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

        // Step 2: Add/Merge nodes in graph
        const today = new Date().toISOString().split('T')[0];
        const extractionNames = extraction.entities.map(e => e.name);

        // Fetch existing nodes to check for conflicts
        const existingResult = await manager.openNodes(extractionNames);
        const existingNodesMap = new Map(
            existingResult.nodes
                .filter(n => extractionNames.includes(n.name))
                .map(n => [n.name, n])
        );

        const nodesToCreate: Node[] = [];
        const nodesToUpdate: Partial<Node>[] = [];

        for (const entity of extraction.entities) {
            const existingNode = existingNodesMap.get(entity.name);

            if (existingNode) {
                // [SMART MERGE]
                // Archive current metadata and add new ones
                const archivedMetadata = existingNode.metadata.map(m =>
                    m.startsWith('[OLD') ? m : `[OLD - ${today}] ${m}`
                );

                nodesToUpdate.push({
                    name: entity.name,
                    metadata: [...archivedMetadata, ...entity.metadata]
                });
            } else {
                // [NEW NODE]
                nodesToCreate.push({
                    type: 'node',
                    name: entity.name,
                    nodeType: entity.nodeType,
                    metadata: entity.metadata,
                });
            }
        }

        // Process Additions
        if (nodesToCreate.length > 0) {
            try {
                const addedNodeResults = await manager.addNodes(nodesToCreate);
                addedNodeResults.forEach(n => addedNodes.push(n.name));
            } catch (nodeError) {
                errors.push(`Failed to add new nodes: ${nodeError}`);
            }
        }

        // Process Updates (Smart Merges)
        if (nodesToUpdate.length > 0) {
            try {
                const updatedNodeResults = await manager.updateNodes(nodesToUpdate);
                updatedNodeResults.forEach(n => addedNodes.push(`${n.name} (Merged)`));
            } catch (updateError) {
                errors.push(`Failed to merge existing nodes: ${updateError}`);
            }
        }

        // Step 3: Generate embeddings for ALL affected entities (new and updated)
        if (args.generateEmbeddings !== false && process.env.OPENAI_API_KEY) {
            const allAffected = [...nodesToCreate, ...nodesToUpdate];
            for (const entity of allAffected) {
                try {
                    // We need the full node data for embedding
                    const fullNode = entity.name && nodesToUpdate.find(u => u.name === entity.name) || entity as Node;
                    const textForEmbedding = analyzer.summarizeForEmbedding(
                        fullNode.name!,
                        fullNode.nodeType || 'entity',
                        fullNode.metadata || []
                    );
                    const embedding = await analyzer.generateEmbedding(textForEmbedding);

                    const vectorRecord: VectorRecord = {
                        id: `${fullNode.name}-${Date.now()}`,
                        text: textForEmbedding,
                        vector: embedding,
                        nodeName: fullNode.name!,
                        nodeType: fullNode.nodeType || 'entity',
                        metadata: JSON.stringify(fullNode.metadata),
                    };

                    await addVector(vectorRecord);
                } catch (embedError) {
                    console.error('[AutoAdd] Embedding error:', embedError);
                }
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

/**
 * Handle hybrid_search tool call
 */
export async function handleHybridSearch(
    args: { query: string; limit?: number; depth?: number },
    manager: ApplicationManager
): Promise<ToolResponse> {
    try {
        const limit = args.limit || 5;
        const depth = args.depth || 1;

        // 1. Keyword search (with BFS)
        const keywordResult = await manager.searchNodes(args.query, depth);

        // 2. Semantic search
        let semanticResults: any[] = [];
        if (process.env.OPENAI_API_KEY) {
            const queryEmbedding = await analyzer.generateEmbedding(args.query);
            semanticResults = await searchVectors(queryEmbedding, limit * 2);
        }

        // 3. Reciprocal Rank Fusion (RRF)
        const scores = new Map<string, number>();
        const nodeData = new Map<string, Node>();

        // Score Keyword results
        keywordResult.nodes.forEach((node, index) => {
            const score = 1 / (60 + (index + 1));
            scores.set(node.name, (scores.get(node.name) || 0) + score);
            nodeData.set(node.name, node);
        });

        // Score Semantic results
        semanticResults.forEach((res, index) => {
            const score = 1 / (60 + (index + 1));
            scores.set(res.nodeName, (scores.get(res.nodeName) || 0) + score);
        });

        // Fetch nodes found by semantic search but not by keyword search
        const missingNames = semanticResults
            .map(r => r.nodeName)
            .filter(name => !nodeData.has(name));

        if (missingNames.length > 0) {
            const extraNodes = await manager.openNodes(missingNames, depth);
            extraNodes.nodes.forEach(n => nodeData.set(n.name, n));
        }

        // Final Sort
        const finalResults = Array.from(scores.entries())
            .sort((a, b) => b[1] - a[1])
            .slice(0, limit)
            .map(([name, score]) => ({
                name,
                score: score.toFixed(4),
                node: nodeData.get(name)
            }));

        return {
            toolResult: {
                isError: false,
                data: finalResults,
                actionTaken: `Hybrid search completed for "${args.query}"`,
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: formatGraphAsNarrative({
                        nodes: finalResults.map(r => r.node!).filter(Boolean),
                        edges: [] // We could potentially pull in edges here too if we wanted deeper narrative
                    })
                }],
            },
        };
    } catch (error) {
        return {
            toolResult: {
                isError: true,
                data: null,
                actionTaken: 'hybrid_search failed',
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: `Error: ${error instanceof Error ? error.message : 'Unknown error'}`,
                }],
            },
        };
    }
}

export const autoMemoryTools = [autoAddMemoryTool, semanticSearchTool, hybridSearchTool];
