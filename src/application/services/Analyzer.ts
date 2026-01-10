import { createOpenAI } from '@ai-sdk/openai';
import { generateText } from 'ai';
import { z } from 'zod';
import 'dotenv/config';

// Schema for extracted entities
const EntitySchema = z.object({
    entities: z.array(z.object({
        name: z.string().describe('Unique name/identifier for the entity'),
        nodeType: z.string().describe('Type of entity (npc, location, artifact, quest, faction, etc.)'),
        metadata: z.array(z.string()).describe('Key-value pairs describing the entity'),
    })),
    relationships: z.array(z.object({
        from: z.string().describe('Source entity name'),
        to: z.string().describe('Target entity name'),
        edgeType: z.string().describe('Type of relationship (owns, located_in, allied_with, etc.)'),
    })),
});

type ExtractionResult = z.infer<typeof EntitySchema>;

// Configure OpenAI client - supports both OpenAI and LM Studio
const openai = createOpenAI({
    apiKey: process.env.OPENAI_API_KEY || 'lm-studio',
    baseURL: process.env.OPENAI_BASE_URL || 'https://api.openai.com/v1',
});

// Model name - use LLM_MODEL env var or default
const modelName = process.env.LLM_MODEL || 'gpt-4o-mini';

/**
 * Analyzer module - extracts entities and relationships from natural language text
 * Uses LLM to understand context and generate graph operations
 * 
 * Supports:
 * - OpenAI API (set OPENAI_API_KEY)
 * - LM Studio (set OPENAI_BASE_URL=http://localhost:1234/v1)
 * - Any OpenAI-compatible API
 */
export class Analyzer {
    private model = openai(modelName);

    /**
     * Extract entities and relationships from text
     */
    async extractFromText(text: string, availableTypes: string[] = []): Promise<ExtractionResult> {
        const typeHint = availableTypes.length > 0
            ? `Available entity types: ${availableTypes.join(', ')}. Use these when appropriate.`
            : 'Common types: npc, location, artifact, quest, faction, player_character, currency, transportation.';

        // Use generateText instead of generateObject for better local compatibility
        const result = await generateText({
            model: this.model,
            prompt: `Analyze the following text and extract entities and relationships.
            
Output ONLY valid JSON matching this structure:
{
  "entities": [
    { "name": "string", "nodeType": "string", "metadata": ["string"] }
  ],
  "relationships": [
    { "from": "string", "to": "string", "edgeType": "string" }
  ]
}

${typeHint}

Common relationship types: located_in, owns, member_of, allied_with, enemy_of, knows, related_to, part_of, started_by, completed_by.

Text to analyze:
"${text}"`,
        });

        try {
            // Robust Parsing: Find the first { and the last }
            const jsonMatch = result.text.match(/\{[\s\S]*\}/);
            const jsonString = jsonMatch ? jsonMatch[0] : result.text;

            // Clean up potentially malformed markdown if regex failed or captured too much
            const cleanJson = jsonString
                .replace(/```json/g, '')
                .replace(/```/g, '')
                .trim();

            const parsed = JSON.parse(cleanJson);

            // Basic validation
            if (!parsed.entities) parsed.entities = [];
            if (!parsed.relationships) parsed.relationships = [];

            return parsed as ExtractionResult;
        } catch (e) {
            // Fallback: If JSON parsing fails, return empty result instead of throwing
            return { entities: [], relationships: [] };
        }
    }

    /**
     * Generate embeddings for text (to be stored in vector DB)
     * Note: LM Studio may not support embeddings - check ENABLE_EMBEDDINGS env var
     */
    async generateEmbedding(text: string): Promise<number[]> {
        // Check if embeddings are disabled
        if (process.env.ENABLE_EMBEDDINGS === 'false') {
            // Return a simple hash-based pseudo-embedding (not for semantic search, just for storage)
            console.error('[Analyzer] Embeddings disabled, using placeholder');
            return this.simpleHashEmbedding(text);
        }

        const baseUrl = process.env.OPENAI_BASE_URL || 'https://api.openai.com/v1';
        const apiKey = process.env.OPENAI_API_KEY || 'lm-studio';

        const response = await fetch(`${baseUrl}/embeddings`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${apiKey}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                model: process.env.EMBEDDING_MODEL || 'text-embedding-3-small',
                input: text,
            }),
        });

        if (!response.ok) {
            console.error('[Analyzer] Embedding API error, using placeholder');
            return this.simpleHashEmbedding(text);
        }

        const data = await response.json() as { data: { embedding: number[] }[] };
        return data.data[0].embedding;
    }

    /**
     * Simple hash-based pseudo-embedding (fallback when embeddings are disabled)
     * This won't provide meaningful semantic search but allows storage
     */
    private simpleHashEmbedding(text: string): number[] {
        const embedding = new Array(384).fill(0);
        for (let i = 0; i < text.length; i++) {
            embedding[i % 384] += text.charCodeAt(i) / 1000;
        }
        // Normalize
        const sum = Math.sqrt(embedding.reduce((a, b) => a + b * b, 0));
        return embedding.map(v => v / (sum || 1));
    }

    /**
     * Summarize metadata for a node
     */
    summarizeForEmbedding(name: string, nodeType: string, metadata: string[]): string {
        return `${nodeType}: ${name}. ${metadata.join('. ')}`;
    }
}

export const analyzer = new Analyzer();
