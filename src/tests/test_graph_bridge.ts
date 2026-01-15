
import { initDatabase, getDatabase, getSqliteInstance, closeDatabase, schema } from '../infrastructure/database/index.js';
import { GraphQueryEngine } from '../core/graph/GraphQueryEngine.js';
import { analyzer } from '../application/services/Analyzer.js';

// Mock Analyzer embedding generation
analyzer.generateEmbedding = async (text) => {
    return new Array(384).fill(0.1);
};

async function testGraphBridge() {
    try {
        console.log('[Test] Init DB...');
        // We ensure DB is clean-ish for this test
        try {
            getSqliteInstance().exec("DELETE FROM nodes WHERE name IN ('Bridge_A', 'Bridge_B')");
            getSqliteInstance().exec("DELETE FROM edges WHERE from_node = 'Bridge_A'");
        } catch (e) {
            initDatabase(); // Initialize if not already
        }

        const db = getDatabase();
        const engine = new GraphQueryEngine();

        // 1. Setup Graph Data
        console.log('[Test] Creating Nodes/Edges...');
        await db.insert(schema.nodes).values([
            { name: 'Bridge_A', nodeType: 'concept', metadata: JSON.stringify(['Start point']) },
            { name: 'Bridge_B', nodeType: 'concept', metadata: JSON.stringify(['End point']) }
        ]);

        await db.insert(schema.edges).values({
            fromNode: 'Bridge_A',
            toNode: 'Bridge_B',
            edgeType: 'leads_to'
        });

        // 2. Mock Vector Search Function
        // We simulate finding 'Bridge_A' via vector search
        const mockSearch = async (queryVec: any, limit: any) => {
            console.log('[Test Mock] searchVectors called.');
            return [
                { nodeName: 'Bridge_A', score: 0.9 }
            ];
        };

        // 3. Run findRelevantSubgraph with INJECTED search
        console.log('[Test] Searching sub-graph...');
        const result = await engine.findRelevantSubgraph("Search query match", 2, mockSearch);

        console.log('[Test] Result Nodes:', result.nodes.map(n => n.name));

        const hasA = result.nodes.find(n => n.name === 'Bridge_A');
        const hasB = result.nodes.find(n => n.name === 'Bridge_B');

        if (hasA && hasB) {
            console.log('SUCCESS: Found Bridge_A (Mock Vector) and Bridge_B (Graph Neighbor)');
        } else {
            console.error('FAILURE: Graph Bridge did not return expected subgraph.');
            process.exit(1);
        }

    } catch (e) {
        console.error('[Test] Failed:', e);
        process.exit(1);
    } finally {
        closeDatabase();
        // Force exit to prevent hanging on open handles
        process.exit(0);
    }
}

testGraphBridge();
