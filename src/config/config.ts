// src/config/config.ts

import path from 'path';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
import { ENV } from './env.js';

/**
 * Helper to get the project root directory
 */
const PROJECT_ROOT = path.resolve(__dirname, '../..');

interface ServerConfig {
    NAME: string;
    VERSION: string;
}

interface PathsConfig {
    PROJECT_ROOT: string;
    DATA_DIR: string;
    SCHEMAS_DIR: string;
    MODULES_DIR: string;

}

interface SchemaConfig {
    SUPPORTED_VERSIONS: string[];
}

interface ModuleConfig {
    ACTIVE: string[];
}

interface SearchConfig {
    RRF_CONSTANT: number;
    DEFAULT_LIMIT: number;
    DEFAULT_DEPTH: number;
    MAX_DEPTH: number;
}

interface ContextConfig {
    L3_MESSAGE_LIMIT: number;
    COMPACTION_THRESHOLD: number;
    TOKEN_LIMIT: number;
    KEEP_RECENT_MESSAGES: number;
    MAX_L3_TOKENS: number;
}

interface EmbeddingsConfig {
    FALLBACK_DIMENSIONS: number;
    BATCH_SIZE: number;
    DEFAULT_MODEL: string;
}

interface LLMConfig {
    DEFAULT_MODEL: string;
    MAX_RETRIES: number;
    RETRY_DELAY_MS: number;
    TIMEOUT_MS: number;
    DEFAULT_BASE_URL: string;
    CHARS_PER_TOKEN: number;
}

interface ValidationConfig {
    MAX_NODE_NAME_LENGTH: number;
    MAX_TEXT_LENGTH: number;
    MAX_METADATA_ITEMS: number;
}

interface RateLimitConfig {
    WINDOW_MS: number;
    MAX_REQUESTS: number;
    BLOCK_DURATION_MS: number;
}

interface Config {
    SERVER: ServerConfig;
    PATHS: PathsConfig;
    SCHEMA: SchemaConfig;
    MODULES: ModuleConfig;
    SEARCH: SearchConfig;
    CONTEXT: ContextConfig;
    EMBEDDINGS: EmbeddingsConfig;
    LLM: LLMConfig;
    VALIDATION: ValidationConfig;
    RATE_LIMIT: RateLimitConfig;
}

/**
 * Centralized configuration for the ReMem Modular Engine.
 */
export const CONFIG: Config = {
    SERVER: {
        NAME: 'remem-engine',
        VERSION: '0.3.0',
    },

    PATHS: {
        PROJECT_ROOT,
        DATA_DIR: path.join(PROJECT_ROOT, 'data'),
        SCHEMAS_DIR: path.join(PROJECT_ROOT, 'data', 'schemas'), // Core schemas
        MODULES_DIR: path.join(PROJECT_ROOT, 'src', 'modules'), // Modular schemas/tools

    },

    SCHEMA: {
        SUPPORTED_VERSIONS: ['0.1', '0.2', '0.3'],
    },

    MODULES: {
        // Load modules from environment or default to common ones
        ACTIVE: ENV.REMEM_MODULES
            ? ENV.REMEM_MODULES.split(',').map(m => m.trim())
            : ['rpg', 'coding'], // Default to both for now, filter logic later
    },

    SEARCH: {
        RRF_CONSTANT: 60, // Reciprocal Rank Fusion constant
        DEFAULT_LIMIT: 5,
        DEFAULT_DEPTH: 1,
        MAX_DEPTH: 10,
    },

    CONTEXT: {
        L3_MESSAGE_LIMIT: 10,
        COMPACTION_THRESHOLD: 50,
        TOKEN_LIMIT: 4000,
        KEEP_RECENT_MESSAGES: 10,
        MAX_L3_TOKENS: 6000,
    },

    EMBEDDINGS: {
        FALLBACK_DIMENSIONS: 384, // Dimension for hash-based fallback embeddings
        BATCH_SIZE: 10, // Number of embeddings to generate in parallel
        DEFAULT_MODEL: 'text-embedding-3-small',
    },

    LLM: {
        DEFAULT_MODEL: ENV.LLM_MODEL,
        MAX_RETRIES: 3,
        RETRY_DELAY_MS: 1000,
        TIMEOUT_MS: 30000,
        DEFAULT_BASE_URL: ENV.OPENAI_BASE_URL || 'https://api.openai.com/v1',
        CHARS_PER_TOKEN: 4,
    },

    VALIDATION: {
        MAX_NODE_NAME_LENGTH: 200,
        MAX_TEXT_LENGTH: 10000,
        MAX_METADATA_ITEMS: 100,
    },

    RATE_LIMIT: {
        WINDOW_MS: 60 * 1000, // 1 minute
        MAX_REQUESTS: 100,    // 100 requests per minute
        BLOCK_DURATION_MS: 5 * 60 * 1000, // 5 minutes block
    },
};