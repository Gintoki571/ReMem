import { analyzer } from '@application/services/Analyzer.js';
import { addVector, searchVectors, deleteVectorsByNode, VectorRecord } from '@infrastructure/vector/VectorManager.js';
import { getDatabase, schema, getSqliteInstance } from '@infrastructure/database/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import { formatGraphAsNarrative } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import { Logger } from '@core/logging/Logger.js';
import type { Node, Edge } from '@core/index.js';
import { retryWithBackoff } from '../../../utils/retryWithBackoff.js';

import { CONFIG } from '@config/config.js';
import { PROMPTS } from '@config/prompts.js';

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
                description: `The text to analyze and extract memory from (max ${CONFIG.VALIDATION.MAX_TEXT_LENGTH} characters)`,
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
        // Input validation
        if (!args.text || typeof args.text !== 'string') {
            return {
                toolResult: {
                    isError: true,
                    content: [{ type: 'text', text: 'Error: text parameter is required and must be a string.' }],
                    timestamp: new Date().toISOString()
                }
            };
        }

        if (args.text.length > CONFIG.VALIDATION.MAX_TEXT_LENGTH) {
            return {
                toolResult: {
                    isError: true,
                    content: [{
                        type: 'text',
                        text: `Error: text exceeds maximum length of ${CONFIG.VALIDATION.MAX_TEXT_LENGTH} characters.`
                    }],
                    timestamp: new Date().toISOString()
                }
            };
        }

        if (args.text.trim().length === 0) {
            return {
                toolResult: {
                    isError: true,
                    content: [{ type: 'text', text: 'Error: text cannot be empty.' }],
                    timestamp: new Date().toISOString()
                }
            };
        }

        // --- Step 0: Fetch Global Context ---
        let globalContext = '';
        try {
            const globalNodes = await manager.searchNodes('global_fact', 10);
            const relevantGlobals = globalNodes.nodes.filter(n => n.nodeType === 'global_fact');
            if (relevantGlobals.length > 0) {
                globalContext = relevantGlobals.map(n => {
                    try {
                        const meta = n.metadata || {};
                        return Object.values(meta).join('. ');
                    } catch { return ''; }
                }).join('. ');
            }
        } catch (e) {
            Logger.warn('AutoAdd', 'Failed to fetch global context', e);
        }

        // --- Step 1: Extract entities using LLM (Read-Only) ---
        const extraction = await analyzer.extractFromText(args.text, [], globalContext);

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

        // --- Step 2: Pre-Generate Embeddings (Fail-Fast Phase) ---
        const embeddingMap = new Map<string, number[]>();
        const canEmbed = process.env.OPENAI_API_KEY || process.env.OPENAI_BASE_URL;

        if (args.generateEmbeddings !== false && canEmbed) {
            for (const entity of extraction.entities) {
                try {
                    const textForEmbedding = analyzer.summarizeForEmbedding(
                        entity.name,
                        entity.nodeType,
                        entity.metadata
                    );
                    const embedding = await analyzer.generateEmbedding(textForEmbedding);
                    embeddingMap.set(entity.name, embedding);
                } catch (embedError) {
                    Logger.error('AutoAdd', `Embedding generation failed for ${entity.name}`, embedError);
                    throw new Error(`Embedding generation failed: ${embedError instanceof Error ? embedError.message : String(embedError)}`);
                }
            }
        }

        // --- Step 3: Atomic Write (SQL + Vector) ---
        const addedNodes: string[] = [];
        const addedEdges: string[] = [];
        const vectorsTrackedForRollback: string[] = [];
        const errors: string[] = [];

        try {
            await manager.withTransaction(async () => {
                const extractionNames = extraction.entities.map(e => e.name);
                const existingResult = await manager.openNodes(extractionNames);
                const existingNodesMap = new Map(existingResult.nodes.map(n => [n.name, n]));

                const nodesToCreate: Node[] = [];
                const nodesToUpdate: Partial<Node>[] = [];

                // 3a. Prepare Node Records
                for (const entity of extraction.entities) {
                    const existingNode = existingNodesMap.get(entity.name);

                    if (existingNode) {
                        const currentFacts = existingNode.metadata ? existingNode.metadata : {};
                        let newMetadata: Record<string, unknown> = { ...currentFacts, ...(entity.metadata || {}) };
                        nodesToUpdate.push({
                            name: entity.name,
                            metadata: newMetadata,
                        });
                    } else {
                        nodesToCreate.push({
                            type: 'node',
                            name: entity.name,
                            nodeType: entity.nodeType,
                            metadata: entity.metadata,
                        });
                    }
                }

                // 3b. Execute SQL Writes
                if (nodesToCreate.length > 0) {
                    const res = await manager.addNodes(nodesToCreate);
                    res.forEach(n => addedNodes.push(n.name));
                }

                if (nodesToUpdate.length > 0) {
                    const res = await manager.updateNodes(nodesToUpdate);
                    res.forEach(n => addedNodes.push(`${n.name} (Merged)`));
                }

                const edgesToAdd: Edge[] = extraction.relationships.map(rel => ({
                    type: 'edge' as const,
                    from: rel.from,
                    to: rel.to,
                    edgeType: rel.edgeType,
                }));

                if (edgesToAdd.length > 0) {
                    const res = await manager.addEdges(edgesToAdd);
                    res.forEach(e => addedEdges.push(`${e.from} -[${e.edgeType}]-> ${e.to}`));
                }

                // 3c. Write Vectors (Inside SQL Transaction)
                if (embeddingMap.size > 0) {
                    for (const entity of extraction.entities) {
                        const embedding = embeddingMap.get(entity.name);
                        if (embedding) {
                            const vectorRecord: VectorRecord = {
                                id: `${entity.name}-${Date.now()}`,
                                text: analyzer.summarizeForEmbedding(entity.name, entity.nodeType, entity.metadata),
                                vector: embedding,
                                nodeName: entity.name,
                                nodeType: entity.nodeType,
                                metadata: (entity.metadata as Record<string, unknown>) || {}
                            };
                            await addVector(vectorRecord);
                            vectorsTrackedForRollback.push(entity.name);
                        }
                    }
                }
            });

        } catch (transactionError) {
            Logger.error('AutoAdd', 'Transaction Failed. Rolling back internal state...', transactionError);

            // COMPENSATE: Cleanup orphaned vectors if SQL rollback happened after vector writes
            if (vectorsTrackedForRollback.length > 0) {
                Logger.warn('AutoAdd', `Cleaning up ${vectorsTrackedForRollback.length} orphaned vectors...`);
                for (const nodeName of vectorsTrackedForRollback) {
                    try {
                        await deleteVectorsByNode(nodeName);
                    } catch (cleanupError) {
                        Logger.error('AutoAdd', `Failed to cleanup vector for ${nodeName}`, cleanupError);
                    }
                }
            }
            throw transactionError;
        }

        return {
            toolResult: {
                isError: false,
                data: {
                    nodesAdded: addedNodes,
                    edgesAdded: addedEdges,
                },
                actionTaken: `Added ${addedNodes.length} entities and ${addedEdges.length} relationships`,
                timestamp: new Date().toISOString(),
                content: [{
                    type: 'text',
                    text: `Successfully added ${addedNodes.length} entities and ${addedEdges.length} relationships.`
                }],
            }
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
        if (!args.query || typeof args.query !== 'string') {
            return {
                toolResult: {
                    isError: true,
                    content: [{ type: 'text', text: 'Error: query parameter is required and must be a string.' }],
                    timestamp: new Date().toISOString()
                }
            };
        }

        const canEmbed = process.env.OPENAI_API_KEY || process.env.OPENAI_BASE_URL;
        if (!canEmbed) {
            return {
                toolResult: {
                    isError: true,
                    data: null,
                    actionTaken: 'semantic_search requires OPENAI_API_KEY or local provider',
                    timestamp: new Date().toISOString(),
                    content: [{
                        type: 'text',
                        text: 'Semantic search requires OPENAI_API_KEY or a local embedding provider (set OPENAI_BASE_URL).',
                    }],
                },
            };
        }

        const queryEmbedding = await analyzer.generateEmbedding(args.query);
        const results = await searchVectors(queryEmbedding, args.limit || 5);

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
        const canEmbed = process.env.OPENAI_API_KEY || process.env.OPENAI_BASE_URL;
        if (canEmbed) {
            const queryEmbedding = await analyzer.generateEmbedding(args.query);
            semanticResults = await searchVectors(queryEmbedding, limit * 2);
        }

        // 3. Reciprocal Rank Fusion (RRF)
        const scores = new Map<string, number>();
        const nodeData = new Map<string, Node>();

        // Score Keyword results
        keywordResult.nodes.forEach((node, index) => {
            const score = 1 / (CONFIG.SEARCH.RRF_CONSTANT + (index + 1));
            scores.set(node.name, (scores.get(node.name) || 0) + score);
            nodeData.set(node.name, node);
        });

        // Score Semantic results
        semanticResults.forEach((res, index) => {
            const score = 1 / (CONFIG.SEARCH.RRF_CONSTANT + (index + 1));
            scores.set(res.nodeName, (scores.get(res.nodeName) || 0) + score);
        });

        const missingNames = semanticResults
            .map(r => r.nodeName)
            .filter(name => !nodeData.has(name));

        if (missingNames.length > 0) {
            const extraNodes = await manager.openNodes(missingNames, depth);
            extraNodes.nodes.forEach(n => nodeData.set(n.name, n));
        }

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
                        edges: []
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
