import { createOpenAI } from '@ai-sdk/openai';
import { generateText } from 'ai';
import { z } from 'zod';
import 'dotenv/config';
import { CONFIG } from '@config/config.js';
import { PROMPTS } from '@config/prompts.js';
import { LLMError } from '@shared/errors/index.js';

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
    baseURL: process.env.OPENAI_BASE_URL || CONFIG.LLM.DEFAULT_BASE_URL,
});

// Model name - use LLM_MODEL env var or default
const modelName = process.env.LLM_MODEL || CONFIG.LLM.DEFAULT_MODEL;

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
    async extractFromText(text: string, availableTypes: string[] = [], globalContext: string = ''): Promise<ExtractionResult> {
        const typeHint = availableTypes.length > 0
            ? `Available entity types: ${availableTypes.join(', ')}. Use these when appropriate.`
            : 'Common types: npc, location, artifact, quest, faction, player_character, currency, transportation.';

        const globalContextSection = globalContext ? PROMPTS.EXTRACTION.GLOBAL_CONTEXT_TEMPLATE(globalContext) : '';

        // Use generateText instead of generateObject for better local compatibility
        const result = await generateText({
            model: this.model,
            prompt: `${PROMPTS.EXTRACTION.SYSTEM}

${globalContextSection}

${typeHint}

Text to analyze:
"${text}"`,
        });


        try {
            // Robust Parsing: Priority 1 - Markdown Code Block
            const codeBlockMatch = result.text.match(/```(?:json)?\s*(\{[\s\S]*?\})\s*```/);

            let jsonString = '';
            if (codeBlockMatch) {
                jsonString = codeBlockMatch[1];
            } else {
                // Priority 2 - First plausible JSON object in text
                const jsonMatch = result.text.match(/\{[\s\S]*\}/);
                jsonString = jsonMatch ? jsonMatch[0] : result.text;
            }

            // Cleanup
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
            // Log the error for debugging
            console.error('[Analyzer] JSON parsing failed for extraction:', e);
            console.error('[Analyzer] Raw LLM response:', result.text.substring(0, 500));

            // Fallback: Return empty result
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
            console.error('[Analyzer] Embeddings disabled, using placeholder');
            return this.simpleHashEmbedding(text);
        }

        const baseUrl = process.env.OPENAI_BASE_URL || 'https://api.openai.com/v1';
        const apiKey = process.env.OPENAI_API_KEY || 'lm-studio';
        const embeddingModel = process.env.EMBEDDING_MODEL || CONFIG.EMBEDDINGS.DEFAULT_MODEL;

        try {
            const response = await fetch(`${baseUrl}/embeddings`, {
                method: 'POST',
                headers: {
                    'Authorization': `Bearer ${apiKey}`,
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    model: embeddingModel,
                    input: text,
                }),
            });

            if (!response.ok) {
                const errorText = await response.text();
                console.error(`[Analyzer] Embedding API error: ${response.status} ${response.statusText}`, errorText);
                throw new LLMError(
                    `Embedding API returned ${response.status}`,
                    embeddingModel,
                    { status: response.status, error: errorText }
                );
            }

            const data = await response.json() as { data: { embedding: number[] }[] };
            return data.data[0].embedding;
        } catch (error) {
            if (error instanceof LLMError) {
                throw error;
            }
            console.error('[Analyzer] Embedding generation failed, using fallback:', error);
            return this.simpleHashEmbedding(text);
        }
    }

    /**
     * Simple hash-based pseudo-embedding (fallback when embeddings are disabled)
     * This won't provide meaningful semantic search but allows storage
     */
    private simpleHashEmbedding(text: string): number[] {
        const embedding = new Array(CONFIG.EMBEDDINGS.FALLBACK_DIMENSIONS).fill(0);
        for (let i = 0; i < text.length; i++) {
            embedding[i % CONFIG.EMBEDDINGS.FALLBACK_DIMENSIONS] += text.charCodeAt(i) / 1000;
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

    /**
     * Summarize a list of messages into a concise narrative
     * (Letta-style Rolling Summary)
     */
    async summarizeMessages(messages: { role: string, content: string }[], previousSummary: string = ''): Promise<string> {
        const messageText = messages.map(m => `${m.role.toUpperCase()}: ${m.content}`).join('\n');

        const prompt = `${PROMPTS.SUMMARIZATION.SYSTEM}

Current Summary of previous history:
"${previousSummary || 'None'}"

New Messages to incorporate:
${messageText}`;

        const result = await generateText({
            model: this.model,
            prompt: prompt,
        });

        return result.text.trim();
    }

    /**
     * Decision logic for Memory Updates (mem0 style)
     * Decides whether to ADD, UPDATE, or DELETE based on new facts vs old memory
     */
    async determineMemoryUpdates(
        newFacts: string[],
        existingMemories: { id: string, text: string }[]
    ): Promise<{ action: 'ADD' | 'UPDATE' | 'DELETE' | 'NONE', id?: string, text: string }[]> {
        const prompt = `${PROMPTS.SMART_MERGE.SYSTEM}

Existing Memories:
${JSON.stringify(existingMemories, null, 2)}

New Facts:
${JSON.stringify(newFacts, null, 2)}`;

        const result = await generateText({
            model: this.model,
            prompt: prompt,
        });

        try {
            // Robust Parsing: Priority 1 - Markdown Code Block (Array)
            const codeBlockMatch = result.text.match(/```(?:json)?\s*(\[\s*[\s\S]*?\s*\])\s*```/);

            let jsonString = '';
            if (codeBlockMatch) {
                jsonString = codeBlockMatch[1];
            } else {
                // Priority 2 - First plausible JSON array
                const jsonMatch = result.text.match(/\[[\s\S]*\]/);
                jsonString = jsonMatch ? jsonMatch[0] : result.text;
            }

            const cleanJson = jsonString.replace(/```json/g, '').replace(/```/g, '').trim();
            return JSON.parse(cleanJson);
        } catch (e) {
            console.error('[Analyzer] Failed to parse memory update decision:', e);
            console.error('[Analyzer] Raw LLM response:', result.text.substring(0, 500));

            // Fallback: Treat all as ADD operations
            return newFacts.map(f => ({ action: 'ADD', text: f }));
        }
    }
}

export const analyzer = new Analyzer();
