// src/core/tokenizer/TokenEstimator.ts

/**
 * TokenEstimator Service
 * 
 * Uses js-tiktoken for accurate GPT-style tokenization.
 * Falls back to character-based heuristics if encoding fails.
 */

import { getEncoding, Tiktoken } from 'js-tiktoken';
import { CONFIG } from '@config/config.js';

// Initialize encoder once (lazy loaded)
let encoder: Tiktoken | null = null;

function getEncoder(): Tiktoken {
    if (!encoder) {
        // cl100k_base is used by GPT-4, GPT-3.5-turbo, and text-embedding-ada-002
        encoder = getEncoding('cl100k_base');
    }
    return encoder;
}

export class TokenEstimator {
    // Fallback: character-based heuristic (~4 chars/token for English)
    private static CHARS_PER_TOKEN = CONFIG.LLM.CHARS_PER_TOKEN;

    /**
     * Counts the exact number of tokens using tiktoken.
     * Falls back to heuristic if encoding fails.
     */
    public static countTokens(text: string): number {
        if (!text) return 0;

        try {
            const enc = getEncoder();
            return enc.encode(text).length;
        } catch (error) {
            // Fallback to heuristic
            console.warn('[TokenEstimator] Encoder failed, using heuristic:', error);
            return Math.ceil(text.length / this.CHARS_PER_TOKEN);
        }
    }

    /**
     * Counts tokens for a list of messages.
     * Includes overhead for message formatting (role labels, separators).
     */
    public static countMessageTokens(messages: { role: string, content: string }[]): number {
        return messages.reduce((acc, msg) => {
            // Content tokens + overhead (role + separators ~ 4 tokens per message)
            return acc + this.countTokens(msg.content) + 4;
        }, 0);
    }

    /**
     * Truncates text to a token limit.
     * Uses binary search for efficiency with accurate token counting.
     */
    public static truncateToTokenLimit(text: string, maxTokens: number): string {
        if (!text) return '';

        const currentTokens = this.countTokens(text);
        if (currentTokens <= maxTokens) return text;

        // Binary search for the right truncation point
        let low = 0;
        let high = text.length;
        let result = '';

        while (low < high) {
            const mid = Math.floor((low + high + 1) / 2);
            const truncated = text.substring(0, mid);
            const tokens = this.countTokens(truncated);

            if (tokens <= maxTokens) {
                result = truncated;
                low = mid;
            } else {
                high = mid - 1;
            }
        }

        return result + '...[truncated]';
    }

}
