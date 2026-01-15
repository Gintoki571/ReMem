// src/config/env.ts

import { z } from 'zod';
import dotenv from 'dotenv';
import path from 'path';

// Load environment variables
dotenv.config();

/**
 * Environment Variable Schema
 * Enforces strict validation for critical configuration credentials.
 */
const envSchema = z.object({
    // Server & Environment
    NODE_ENV: z.enum(['development', 'production', 'test']).default('development'),

    // LLM Configuration
    // OPENAI_API_KEY is optional IF we are using a local base URL (like LM Studio)
    // But if provided, it should look like a key (sk-...)
    OPENAI_API_KEY: z.string().optional()
        .refine(val => {
            if (!val) return true; // Optional
            return val.startsWith('sk-') || val === 'lm-studio';
        }, { message: "OPENAI_API_KEY must start with 'sk-' or be 'lm-studio'" }),

    OPENAI_BASE_URL: z.string().url().optional(),

    LLM_MODEL: z.string().default('gpt-4o-mini'),

    // Memory Strategy:
    // 'smart': Use LLM to merge/reason about memories (mem0 style)
    // 'append': Simply append new facts to metadata (Fast, 0 tokens)
    // 'overwrite': Replace metadata with newest facts
    MEMORY_STRATEGY: z.enum(['smart', 'append', 'overwrite']).default('smart'),

    // Optional MCP API Key for authentication
    // If set, all MCP requests must include this key in the 'x-api-key' header
    MCP_API_KEY: z.string().optional(),

    // Default user ID for multi-tenancy (if not provided in requests)
    DEFAULT_USER_ID: z.string().default('default'),

    // Optional modules
    REMEM_MODULES: z.string().optional().default('rpg,coding'),
});

// Process and validate
const _env = envSchema.parse(process.env);

// Custom validation logic for dependencies
// If Base URL is NOT set (defaulting to OpenAI), then API Key MUST be present and valid
if (!_env.OPENAI_BASE_URL && (!_env.OPENAI_API_KEY || _env.OPENAI_API_KEY === 'lm-studio')) {
    if (_env.NODE_ENV !== 'test') { // Skip for tests to avoid annoyance
        console.warn('⚠️  WARNING: No OPENAI_BASE_URL provided, defaulting to OpenAI API, but OPENAI_API_KEY is missing or invalid.');
    }
}

export const ENV = _env;
