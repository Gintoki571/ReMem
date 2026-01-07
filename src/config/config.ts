// src/config/config.ts

import path from 'path';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

/**
 * Helper to get the project root directory
 * Since this file is in src/config/ or dist/config/, 
 * the root is two levels up.
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
    MEMORY_FILE: string;
}

interface SchemaConfig {
    SUPPORTED_VERSIONS: string[];
}

interface Config {
    SERVER: ServerConfig;
    PATHS: PathsConfig;
    SCHEMA: SchemaConfig;
}

/**
 * Centralized configuration for MemoryMesh.
 */
export const CONFIG: Config = {
    SERVER: {
        NAME: 'memorymesh',
        VERSION: '0.2.8',
    },

    PATHS: {
        PROJECT_ROOT,
        /** Root data directory */
        DATA_DIR: path.join(PROJECT_ROOT, 'data'),
        /** Path to schema files directory. */
        SCHEMAS_DIR: path.join(PROJECT_ROOT, 'src', 'data', 'schemas'),
        /** Path to the memory JSON file. */
        MEMORY_FILE: path.join(PROJECT_ROOT, 'data', 'memory.json'),
    },

    SCHEMA: {
        /** Supported schema versions (not yet implemented). */
        SUPPORTED_VERSIONS: ['0.1', '0.2'], // TODO: Add schema versioning
    },
};