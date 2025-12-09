import { sqliteTable, text, integer, real } from 'drizzle-orm/sqlite-core';

// Nodes table - stores knowledge graph nodes
export const nodes = sqliteTable('nodes', {
    id: integer('id').primaryKey({ autoIncrement: true }),
    name: text('name').notNull().unique(),
    nodeType: text('node_type').notNull(),
    metadata: text('metadata'), // JSON string array
    createdAt: integer('created_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
    updatedAt: integer('updated_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
});

// Edges table - stores relationships between nodes
export const edges = sqliteTable('edges', {
    id: integer('id').primaryKey({ autoIncrement: true }),
    fromNode: text('from_node').notNull().references(() => nodes.name),
    toNode: text('to_node').notNull().references(() => nodes.name),
    edgeType: text('edge_type').notNull(),
    weight: real('weight').default(1.0),
    createdAt: integer('created_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
});

// Vector embeddings reference table (actual vectors stored in LanceDB)
export const embeddings = sqliteTable('embeddings', {
    id: integer('id').primaryKey({ autoIncrement: true }),
    nodeName: text('node_name').notNull().references(() => nodes.name),
    embeddingId: text('embedding_id').notNull(), // Reference to LanceDB vector ID
    textContent: text('text_content').notNull(), // Original text that was embedded
    createdAt: integer('created_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
});

// Types for TypeScript
export type Node = typeof nodes.$inferSelect;
export type NewNode = typeof nodes.$inferInsert;
export type Edge = typeof edges.$inferSelect;
export type NewEdge = typeof edges.$inferInsert;
export type Embedding = typeof embeddings.$inferSelect;
export type NewEmbedding = typeof embeddings.$inferInsert;
