// src/config/config.ts

import path from 'path';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

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
    MEMORY_FILE: string;
}

interface SchemaConfig {
    SUPPORTED_VERSIONS: string[];
}

interface ModuleConfig {
    ACTIVE: string[];
}

interface Config {
    SERVER: ServerConfig;
    PATHS: PathsConfig;
    SCHEMA: SchemaConfig;
    MODULES: ModuleConfig;
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
        MEMORY_FILE: path.join(PROJECT_ROOT, 'data', 'memory.json'),
    },

    SCHEMA: {
        SUPPORTED_VERSIONS: ['0.1', '0.2', '0.3'],
    },

    MODULES: {
        // Load modules from environment or default to common ones
        ACTIVE: process.env.REMEM_MODULES
            ? process.env.REMEM_MODULES.split(',').map(m => m.trim())
            : ['rpg', 'coding'], // Default to both for now, filter logic later
    },
};