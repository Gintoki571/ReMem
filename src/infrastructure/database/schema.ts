import { sqliteTable, text, integer, real } from 'drizzle-orm/sqlite-core';

// Nodes table - stores knowledge graph nodes
export const nodes = sqliteTable('nodes', {
    id: integer('id').primaryKey({ autoIncrement: true }),
    name: text('name').notNull().unique(),
    nodeType: text('node_type').notNull(),
    metadata: text('metadata'), // JSON string array
    version: integer('version').default(1).notNull(), // Optimistic Locking
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

// Messages table - stores conversation history for context management (Letta style)
export const messages = sqliteTable('messages', {
    id: integer('id').primaryKey({ autoIncrement: true }),
    role: text('role').notNull(), // 'user' | 'assistant' | 'system'
    content: text('content').notNull(),
    tokenCount: integer('token_count'), // Approx token count
    isSummarized: integer('is_summarized', { mode: 'boolean' }).default(false), // True if this message has been compressed into the summary
    createdAt: integer('created_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
});

// Global State table - stores the current "Rolling Summary" and other singleton data
export const globalState = sqliteTable('global_state', {
    key: text('key').primaryKey(),
    content: text('content').notNull(),
    isSummarized: integer('is_summarized', { mode: 'boolean' }).default(false),
    createdAt: integer('created_at', { mode: 'timestamp' }).$defaultFn(() => new Date()),
    updatedAt: integer('updated_at', { mode: 'timestamp' }).notNull().$defaultFn(() => new Date()),
});

// Types for TypeScript
export type Node = typeof nodes.$inferSelect;
export type NewNode = typeof nodes.$inferInsert;
export type Edge = typeof edges.$inferSelect;
export type NewEdge = typeof edges.$inferInsert;
export type Embedding = typeof embeddings.$inferSelect;
export type NewEmbedding = typeof embeddings.$inferInsert;
export type Message = typeof messages.$inferSelect;
export type NewMessage = typeof messages.$inferInsert;
export type GlobalState = typeof globalState.$inferSelect;
