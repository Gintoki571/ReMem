
/**
 * TokenEstimator Service
 * 
 * A lightweight alternative to heavy WASM tokenizers (tiktoken/transformers.js).
 * Uses character-to-token heuristics which are sufficient for context window management.
 * 
 * Heuristic: ~4 characters per token for English text.
 */

import { CONFIG } from '@config/config.js';

export class TokenEstimator {
    private static CHARS_PER_TOKEN = CONFIG.LLM.CHARS_PER_TOKEN;

    /**
     * Estimates the number of tokens in a string.
     */
    public static countTokens(text: string): number {
        if (!text) return 0;
        return Math.ceil(text.length / this.CHARS_PER_TOKEN);
    }

    /**
     * Estimates tokens for a list of messages.
     * Includes overhead for message formatting (role labels, newlines).
     */
    public static countMessageTokens(messages: { role: string, content: string }[]): number {
        return messages.reduce((acc, msg) => {
            // Add tokens for content + overhead (role + formatting ~ 4 tokens)
            return acc + this.countTokens(msg.content) + 4;
        }, 0);
    }

    /**
     * Truncates a string to a rough token limit.
     */
    public static truncateToTokenLimit(text: string, maxTokens: number): string {
        const estimatedChars = maxTokens * this.CHARS_PER_TOKEN;
        if (text.length <= estimatedChars) return text;

        return text.substring(0, estimatedChars) + '...[truncated]';
    }
}
