import { getSqliteInstance } from '@infrastructure/database/index.js';
import type { Node } from './Node.js';
import type { Edge } from './Edge.js';

export interface GraphPath {
    nodes: Node[];
    edges: Edge[];
}

export class GraphQueryEngine {
    /**
     * Finds all nodes related to a starting node up to a certain depth.
     * Uses Recursive Common Table Expressions (CTEs) for efficient traversal.
     */
    public findRelated(startNodeName: string, maxDepth: number = 2): { nodes: Node[], edges: Edge[] } {
        const db = getSqliteInstance();

        // 1. Recursive CTE to find related nodes and edges
        const query = `
            WITH RECURSIVE traverse(node_name, depth, path) AS (
                -- Base case: Start with the given node
                SELECT name, 0, name
                FROM nodes 
                WHERE name = ?
                
                UNION ALL
                
                -- Recursive step: Find neighbors
                SELECT 
                    CASE 
                        WHEN e.from_node = t.node_name THEN e.to_node 
                        ELSE e.from_node 
                    END,
                    t.depth + 1,
                    t.path || '->' || CASE 
                        WHEN e.from_node = t.node_name THEN e.to_node 
                        ELSE e.from_node 
                    END
                FROM traverse t
                JOIN edges e ON (e.from_node = t.node_name OR e.to_node = t.node_name)
                WHERE t.depth < ?
            )
            SELECT DISTINCT node_name FROM traverse;
        `;

        const relatedNodeNames = db.prepare(query).all(startNodeName, maxDepth) as { node_name: string }[];
        const names = relatedNodeNames.map(r => r.node_name);

        if (names.length === 0) {
            return { nodes: [], edges: [] };
        }

        // 2. Fetch full Node objects
        const nodesQuery = `SELECT * FROM nodes WHERE name IN (${names.map(() => '?').join(',')})`;
        const nodes = db.prepare(nodesQuery).all(...names) as any[];

        // 3. Fetch connecting edges
        const edgesQuery = `
            SELECT * FROM edges 
            WHERE from_node IN (${names.map(() => '?').join(',')}) 
            AND to_node IN (${names.map(() => '?').join(',')})
        `;
        const edges = db.prepare(edgesQuery).all(...names, ...names) as any[];

        return {
            nodes: nodes.map(this.mapNode),
            edges: edges.map(this.mapEdge)
        };
    }

    /**
     * Finds a path between two nodes if one exists.
     */
    public findPath(startNode: string, endNode: string, maxDepth: number = 4): GraphPath | null {
        const db = getSqliteInstance();

        // Recursive CTE to search for a path
        const query = `
            WITH RECURSIVE traverse(current_node, depth, path_string) AS (
                SELECT name, 0, name
                FROM nodes 
                WHERE name = ?
                
                UNION ALL
                
                SELECT 
                    CASE 
                        WHEN e.from_node = t.current_node THEN e.to_node 
                        ELSE e.from_node 
                    END,
                    t.depth + 1,
                    t.path_string || ',' || CASE 
                        WHEN e.from_node = t.current_node THEN e.to_node 
                        ELSE e.from_node 
                    END
                FROM traverse t
                JOIN edges e ON (e.from_node = t.current_node OR e.to_node = t.current_node)
                WHERE t.depth < ? 
                AND instr(t.path_string, CASE 
                        WHEN e.from_node = t.current_node THEN e.to_node 
                        ELSE e.from_node 
                    END) = 0 -- Prevent cycles
            )
            SELECT path_string FROM traverse WHERE current_node = ? LIMIT 1;
        `;

        const result = db.prepare(query).get(startNode, maxDepth, endNode) as { path_string: string } | undefined;

        if (!result) return null;

        const nodeNames = result.path_string.split(',');

        // Fetch full objects for the path
        const nodesq = `SELECT * FROM nodes WHERE name IN (${nodeNames.map(() => '?').join(',')})`;
        const nodes = db.prepare(nodesq).all(...nodeNames) as any[];

        // Sort nodes to match path order
        const sortedNodes = nodeNames.map(name => nodes.find(n => n.name === name)).filter(n => n);

        return {
            nodes: sortedNodes.map(this.mapNode),
            edges: [] // Simplified for now, can fetch edges if needed
        };
    }

    private mapNode(row: any): Node {
        return {
            type: 'node',
            name: row.name,
            nodeType: row.node_type,
            metadata: (row.metadata as Record<string, unknown>) || {}
        };
    }

    private mapEdge(row: any): Edge {
        return {
            type: 'edge',
            from: row.from_node,
            to: row.to_node,
            edgeType: row.edge_type,
            weight: row.weight
        };
    }

    /**
     * Bridges Vector Search and Graph Traversal.
     * 1. Uses Vector Search to find "Entry Nodes" similar to the query.
     * 2. Explores the graph starting from those nodes.
     * 3. Returns a subgraph context.
     * 
     * @param query The search text
     * @param maxDepth Graph traversal depth
     * @param searchFn Optional override for vector search (for testing)
     */
    public async findRelevantSubgraph(
        query: string,
        maxDepth: number = 2,
        searchFn?: (q: number[], limit: number) => Promise<any[]>
    ): Promise<{ nodes: Node[], edges: Edge[] }> {
        // Dynamic import to avoid circular dependency if VectorManager imports Analysis/Graph
        const { searchVectors } = await import('@infrastructure/vector/VectorManager.js');
        const { analyzer } = await import('@application/services/Analyzer.js');

        // Use injected function or default
        const performSearch = searchFn || searchVectors;

        // 1. Get Entry Points via Vector Search
        // We need an embedding for the query.
        let vectorResults: any[] = [];
        try {
            const queryEmbedding = await analyzer.generateEmbedding(query);
            vectorResults = await performSearch(queryEmbedding, 3); // Get top 3 entry points
        } catch (e) {
            console.error('[GraphQueryEngine] Vector search failed (resilience fallback):', e);
            // Fallback: We could do keyword search here if we had access to full DB text search, 
            // but for now we return empty to avoid crash.
            return { nodes: [], edges: [] };
        }

        if (vectorResults.length === 0) {
            return { nodes: [], edges: [] };
        }

        const entryNodeNames = vectorResults.map(r => r.nodeName);
        console.error(`[GraphQueryEngine] Entry points found for "${query}":`, entryNodeNames);

        // 2. Traverse Graph from each entry point
        const allNodes = new Map<string, Node>();
        const allEdges: Edge[] = [];

        for (const startNode of entryNodeNames) {
            const subgraph = this.findRelated(startNode, maxDepth);

            subgraph.nodes.forEach(n => allNodes.set(n.name, n));
            subgraph.edges.forEach(e => allEdges.push(e));
        }

        // 3. Deduplicate Edges (simple string check)
        const uniqueEdges = allEdges.filter((e, index, self) =>
            index === self.findIndex((t) => (
                t.from === e.from && t.to === e.to && t.edgeType === e.edgeType
            ))
        );

        return {
            nodes: Array.from(allNodes.values()),
            edges: uniqueEdges
        };
    }
}
