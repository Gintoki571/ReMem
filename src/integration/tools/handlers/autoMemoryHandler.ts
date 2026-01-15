import { analyzer } from '@application/services/Analyzer.js';
import { addVector, searchVectors, VectorRecord } from '@infrastructure/vector/VectorManager.js';
import { getDatabase, schema, getSqliteInstance } from '@infrastructure/database/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import { formatGraphAsNarrative } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import type { Node, Edge } from '@core/index.js';

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
/**
 * Handle auto_add_memory tool call
 */
export async function handleAutoAddMemory(
    args: { text: string; generateEmbeddings?: boolean },
    manager: ApplicationManager
): Promise<ToolResponse> {
    try {
        const db = getDatabase();

        // --- Step 0: Fetch Global Context (Who is the user?) ---
        // We do this OUTSIDE the transaction because it's a read-only helper
        let globalContext = '';
        try {
            const globalNodes = await manager.searchNodes('global_fact', 10); // heuristic fetch
            const relevantGlobals = globalNodes.nodes.filter(n => n.nodeType === 'global_fact');
            if (relevantGlobals.length > 0) {
                globalContext = relevantGlobals.map(n => {
                    try {
                        const meta = JSON.parse(n.metadata?.[0] || '{}');
                        return meta.content || '';
                    } catch { return ''; }
                }).join('. ');
            }
        } catch (e) {
            console.warn('[AutoAdd] Failed to fetch global context:', e);
        }

        // --- Step 1: Extract entities using LLM (Think Phase) ---
        // console.error('[AutoAdd] Starting extraction for text:', args.text.substring(0, 50) + '...');
        const extraction = await analyzer.extractFromText(args.text, [], globalContext);
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

        // --- Step 2: Critical Section (Write Phase) ---
        // We use a manual Saga pattern here:
        // 1. Write SQL (Nodes/Edges) in strict transaction
        // 2. Generate & Write Embeddings
        // 3. If Embeddings fail -> COMPENSATE by deleting SQL Nodes (Rollback)

        const addedNodes: string[] = [];
        const addedEdges: string[] = [];
        const errors: string[] = [];
        let nodesToRollback: string[] = [];

        try {
            // PART A: SQL Transaction
            await manager.withTransaction(async () => {
                const extractionNames = extraction.entities.map(e => e.name);

                // Fetch existing nodes to check for conflicts AND VERSIONS
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
                        // [OPTIMISTIC LOCKING CHECK]
                        // Ideally we pass 'expectedVersion' if we were an API, but here we are the agent.
                        // We READ just now, so we are safe assuming 'existingNode.version' is current 
                        // UNLESS high concurrency.
                        // For now, we will increment version.

                        // [SMART MERGE]
                        const currentFacts = existingNode.metadata ? existingNode.metadata : [];
                        let newMetadata = [...currentFacts];

                        if (entity.metadata && entity.metadata.length > 0) {
                            try {
                                const updates = await analyzer.determineMemoryUpdates(
                                    entity.metadata,
                                    currentFacts.map((text, idx) => ({ id: idx.toString(), text }))
                                );

                                // Apply updates
                                const tempMap = new Map(currentFacts.map((text, idx) => [idx.toString(), text]));

                                updates.forEach(op => {
                                    if (op.action === 'ADD') {
                                        tempMap.set(`new-${Date.now()}-${Math.random()}`, op.text);
                                    } else if (op.action === 'UPDATE' && op.id && tempMap.has(op.id)) {
                                        tempMap.set(op.id, op.text);
                                    } else if (op.action === 'DELETE' && op.id && tempMap.has(op.id)) {
                                        tempMap.delete(op.id);
                                    }
                                });

                                newMetadata = Array.from(tempMap.values());
                            } catch (e) {
                                console.error('[AutoAdd] Smart merge failed, falling back to append:', e);
                                newMetadata = [...currentFacts, ...entity.metadata];
                            }
                        }

                        nodesToUpdate.push({
                            name: entity.name,
                            metadata: newMetadata,
                            // Drizzle/SQLite helper would need to handle "set version = version + 1"
                            // For now we just pass the new object, the NodeManager needs to handle version increment
                        });
                    } else {
                        // [NEW NODE]
                        nodesToCreate.push({
                            type: 'node',
                            name: entity.name,
                            nodeType: entity.nodeType,
                            metadata: entity.metadata,
                            // version defaults to 1
                        });
                    }
                }

                // Execute SQL Updates
                // NOTE: We assume manager methods participate in the transaction context 
                // (This is a simplification; in a real app, we'd pass 'tx' to manager methods)

                if (nodesToCreate.length > 0) {
                    const res = await manager.addNodes(nodesToCreate);
                    res.forEach(n => {
                        addedNodes.push(n.name);
                        nodesToRollback.push(n.name); // Track for potential rollback
                    });
                }

                if (nodesToUpdate.length > 0) {
                    const res = await manager.updateNodes(nodesToUpdate);
                    res.forEach(n => addedNodes.push(`${n.name} (Merged)`));
                }

                // Edges
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
            });

            // PART B: Vector Embeddings (Outside SQL Transaction, but part of SAGA)
            const canEmbed = process.env.OPENAI_API_KEY || process.env.OPENAI_BASE_URL;
            if (args.generateEmbeddings !== false && canEmbed) {
                // We re-fetch to get cleaner state or just use what we have. 
                // For simplicity, we iterate what we just touched.
                const allNames = extraction.entities.map(e => e.name);

                // We need to fetch the LATEST state to ensure embedding matches DB
                const freshNodes = await manager.openNodes(allNames);

                for (const node of freshNodes.nodes) {
                    try {
                        const textForEmbedding = analyzer.summarizeForEmbedding(
                            node.name,
                            node.nodeType,
                            node.metadata || []
                        );
                        const embedding = await analyzer.generateEmbedding(textForEmbedding);
                        const vectorRecord: VectorRecord = {
                            id: `${node.name}-${Date.now()}`, // Simple versioning for vector
                            text: textForEmbedding,
                            vector: embedding,
                            nodeName: node.name,
                            nodeType: node.nodeType,
                            metadata: JSON.stringify(node.metadata),
                        };
                        await addVector(vectorRecord);
                    } catch (embedError) {
                        console.error(`[AutoAdd] Embedding failed for ${node.name}`, embedError);
                        // CRITICAL: SAGA ROLLBACK TRIGGER
                        throw new Error(`Embedding failed for ${node.name}: ${embedError}`);
                    }
                }
            }

        } catch (transactionError) {
            console.error('[AutoAdd] Transaction Failed. Initiating Rollback...', transactionError);

            // ROLLBACK: Delete the nodes we created to avoid "Zombie Memories" (Nodes without Vectors)
            if (nodesToRollback.length > 0) {
                try {
                    console.error(`[AutoAdd] Rolling back ${nodesToRollback.length} nodes...`);
                    // We assume deleteNodes is available or we use a raw query
                    // manager.deleteNodes(nodesToRollback) - assuming this exists or similar
                    // For now logging it as a TODO since deleteNodes tool exists but maybe not manager method directly exposed?
                    // Actually manager has 'deleteNodes' if we look at similar patterns, or we use db directly.

                    // EMERGENCY CLEANUP
                    const db = getSqliteInstance();
                    const placeholders = nodesToRollback.map(() => '?').join(',');
                    db.prepare(`DELETE FROM nodes WHERE name IN (${placeholders})`).run(...nodesToRollback);
                    db.prepare(`DELETE FROM edges WHERE from_node IN (${placeholders}) OR to_node IN (${placeholders})`).run(...nodesToRollback, ...nodesToRollback);

                    console.error('[AutoAdd] Rollback successful.');
                } catch (rollbackError) {
                    console.error('[AutoAdd] Rollback FAILED. Data corruption possible.', rollbackError);
                }
            }

            throw transactionError; // Re-throw to inform user
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
