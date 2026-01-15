
import { initDatabase, getSqliteInstance, closeDatabase } from '../infrastructure/database/index.js';
import { GraphQueryEngine } from '../core/graph/GraphQueryEngine.js';

async function testTraversals() {
    console.log('Initializing Database...');
    initDatabase();
    const db = getSqliteInstance();

    // Clear existing data for clean test
    console.log('Clearing existing graph data...');
    db.exec('DELETE FROM embeddings');
    db.exec('DELETE FROM edges');
    db.exec('DELETE FROM nodes');

    // Create Test Graph
    // A -> B -> C -> D
    // A -> E -> F
    // B -> F
    console.log('Creating Test Graph...');

    const insertNode = db.prepare('INSERT INTO nodes (name, node_type, metadata) VALUES (?, ?, ?)');
    const insertEdge = db.prepare('INSERT INTO edges (from_node, to_node, edge_type) VALUES (?, ?, ?)');

    const nodes = ['A', 'B', 'C', 'D', 'E', 'F'];
    nodes.forEach(n => insertNode.run(n, 'test_node', '{}'));

    const edges = [
        ['A', 'B'],
        ['B', 'C'],
        ['C', 'D'],
        ['A', 'E'],
        ['E', 'F'],
        ['B', 'F']
    ];
    edges.forEach(([from, to]) => insertEdge.run(from, to, 'connected_to'));

    const engine = new GraphQueryEngine();

    // Test 1: Find Related (Depth 1)
    console.log('\n--- Test 1: Find Related to A (Depth 1) ---');
    const related1 = engine.findRelated('A', 1);
    console.log('Nodes found:', related1.nodes.map(n => n.name).join(', '));
    // Expected: A, B, E

    // Test 2: Find Related (Depth 2)
    console.log('\n--- Test 2: Find Related to A (Depth 2) ---');
    const related2 = engine.findRelated('A', 2);
    console.log('Nodes found:', related2.nodes.map(n => n.name).join(', '));
    // Expected: A, B, E, C, F

    // Test 3: Find Path (A -> D)
    console.log('\n--- Test 3: Find Path A -> D ---');
    const pathAD = engine.findPath('A', 'D', 5);
    if (pathAD) {
        console.log('Path found:', pathAD.nodes.map(n => n.name).join(' -> '));
    } else {
        console.log('No path found');
    }

    // Test 4: Find Path (A -> F via B)
    // Note: BFS might find A -> E -> F or A -> B -> F depending on DB order
    console.log('\n--- Test 4: Find Path A -> F ---');
    const pathAF = engine.findPath('A', 'F', 5);
    if (pathAF) {
        console.log('Path found:', pathAF.nodes.map(n => n.name).join(' -> '));
    } else {
        console.log('No path found');
    }

    closeDatabase();
}

testTraversals().catch(console.error);
