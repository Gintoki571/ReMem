import { z } from 'zod';

export const NodeSchema = z.object({
    type: z.literal('node'),
    name: z.string().min(1, 'Node name cannot be empty').max(200, 'Node name too long'),
    nodeType: z.string().min(1, 'Node type cannot be empty').max(100, 'Node type too long'),
    metadata: z.array(z.string()).default([]),
});

export const EdgeSchema = z.object({
    type: z.literal('edge'),
    from: z.string().min(1, 'Source node name cannot be empty'),
    to: z.string().min(1, 'Target node name cannot be empty'),
    edgeType: z.string().min(1, 'Edge type cannot be empty').max(100, 'Edge type too long'),
    weight: z.number().min(0).max(1).optional(),
});

export const MetadataAdditionSchema = z.object({
    nodeName: z.string().min(1, 'Node name cannot be empty'),
    metadata: z.array(z.string()).min(1, 'At least one metadata entry required'),
});

export const MetadataDeletionSchema = z.object({
    nodeName: z.string().min(1, 'Node name cannot be empty'),
    metadata: z.array(z.string()).min(1, 'At least one metadata entry required'),
});

export const SearchQuerySchema = z.object({
    query: z.string().min(1, 'Search query cannot be empty'),
    depth: z.number().int().min(0).max(10).optional(),
});

export const AutoAddMemorySchema = z.object({
    text: z.string().min(1, 'Text cannot be empty').max(10000, 'Text too long'),
    generateEmbeddings: z.boolean().optional().default(true),
});

export const SemanticSearchSchema = z.object({
    query: z.string().min(1, 'Query cannot be empty'),
    limit: z.number().int().min(1).max(100).optional().default(5),
});

export const HybridSearchSchema = z.object({
    query: z.string().min(1, 'Query cannot be empty'),
    limit: z.number().int().min(1).max(100).optional().default(5),
    depth: z.number().int().min(0).max(10).optional().default(1),
});

export const SqlQuerySchema = z.object({
    query: z.string().min(1, 'SQL query cannot be empty'),
});

export const ModuleNameSchema = z.object({
    moduleName: z.string().min(1, 'Module name cannot be empty'),
});

export const ContextUserIdSchema = z.object({
    userId: z.string().optional().default('default'),
});

export const GlobalMemorySchema = z.object({
    content: z.string().min(1, 'Content cannot be empty'),
    scope: z.enum(['system', 'user', 'session']).default('system'),
});

export type Node = z.infer<typeof NodeSchema>;
export type Edge = z.infer<typeof EdgeSchema>;
export type MetadataAddition = z.infer<typeof MetadataAdditionSchema>;
export type MetadataDeletion = z.infer<typeof MetadataDeletionSchema>;
export type SearchQuery = z.infer<typeof SearchQuerySchema>;
export type AutoAddMemory = z.infer<typeof AutoAddMemorySchema>;
export type SemanticSearch = z.infer<typeof SemanticSearchSchema>;
export type HybridSearch = z.infer<typeof HybridSearchSchema>;
export type SqlQuery = z.infer<typeof SqlQuerySchema>;
export type ModuleName = z.infer<typeof ModuleNameSchema>;
export type ContextUserId = z.infer<typeof ContextUserIdSchema>;
export type GlobalMemory = z.infer<typeof GlobalMemorySchema>;

export class ValidationError extends Error {
    constructor(
        message: string,
        public field?: string,
        public details?: Record<string, any>
    ) {
        super(message);
        this.name = 'ValidationError';
    }
}

export function validateInput<T>(schema: z.ZodSchema<T>, data: unknown): T {
    try {
        return schema.parse(data);
    } catch (error) {
        if (error instanceof z.ZodError) {
            const firstError = error.errors[0];
            throw new ValidationError(
                firstError.message,
                firstError.path.join('.'),
                { zodErrors: error.errors }
            );
        }
        throw error;
    }
}
