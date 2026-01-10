// src/core/schema/SchemaLoader.ts

import { promises as fs } from 'fs';
import { SchemaBuilder } from './SchemaBuilder.js';
import path from 'path';
import { CONFIG } from '@config/index.js';

interface RawSchemaProperty {
    type: string;
    description: string;
    required?: boolean;
    enum?: string[];
    relationship?: {
        edgeType: string;
        description: string;
        nodeType?: string;
    };
}

interface RawSchema {
    name: string;
    description: string;
    properties: Record<string, RawSchemaProperty>;
    additionalProperties?: boolean;
}

/**
 * Responsible for loading and converting schema definitions from core and modules.
 */
export class SchemaLoader {
    /**
     * Finds the file path for a schema, searching core then modules.
     */
    private static async findSchemaPath(schemaName: string): Promise<string> {
        // 1. Check Core
        const corePath = path.join(CONFIG.PATHS.SCHEMAS_DIR, `${schemaName}.schema.json`);
        try {
            await fs.access(corePath);
            return corePath;
        } catch {
            // Not in core
        }

        // 2. Check Active Modules
        for (const module of CONFIG.MODULES.ACTIVE) {
            const modulePath = path.join(CONFIG.PATHS.MODULES_DIR, module, 'schemas', `${schemaName}.schema.json`);
            try {
                await fs.access(modulePath);
                return modulePath;
            } catch {
                continue;
            }
        }

        throw new Error(`Schema not found: ${schemaName}`);
    }

    /**
     * Loads a specific schema by name.
     */
    static async loadSchema(schemaName: string): Promise<SchemaBuilder> {
        try {
            const schemaPath = await this.findSchemaPath(schemaName);
            const schemaContent = await fs.readFile(schemaPath, 'utf-8');
            const schema = JSON.parse(schemaContent) as RawSchema;
            this.validateSchema(schema);
            return this.convertToSchemaBuilder(schema);
        } catch (error) {
            if (error instanceof Error) {
                throw new Error(`Failed to load schema ${schemaName}: ${error.message}`);
            }
            throw new Error(`Failed to load schema ${schemaName}`);
        }
    }

    /**
     * Converts a JSON schema object into a SchemaBuilder instance.
     */
    static convertToSchemaBuilder(schema: RawSchema): SchemaBuilder {
        const builder = new SchemaBuilder(schema.name, schema.description);

        Object.entries(schema.properties).forEach(([propName, propConfig]) => {
            if (propConfig.type === 'array') {
                builder.addArrayProperty(
                    propName,
                    propConfig.description,
                    propConfig.required,
                    propConfig.enum
                );
            } else {
                builder.addStringProperty(
                    propName,
                    propConfig.description,
                    propConfig.required,
                    propConfig.enum
                );
            }

            if (propConfig.relationship) {
                builder.addRelationship(
                    propName,
                    propConfig.relationship.edgeType,
                    propConfig.relationship.description,
                    propConfig.relationship.nodeType
                );
            }
        });

        if (schema.additionalProperties !== undefined) {
            builder.allowAdditionalProperties(schema.additionalProperties);
        }

        return builder;
    }

    /**
     * Loads ALL schemas from Core and ALL Active Modules.
     */
    static async loadAllSchemas(): Promise<Record<string, SchemaBuilder>> {
        const schemas: Record<string, SchemaBuilder> = {};

        // 1. Load Core
        await this.loadFromDir(CONFIG.PATHS.SCHEMAS_DIR, schemas);

        // 2. Load Modules
        for (const module of CONFIG.MODULES.ACTIVE) {
            const moduleSchemaDir = path.join(CONFIG.PATHS.MODULES_DIR, module, 'schemas');
            await this.loadFromDir(moduleSchemaDir, schemas);
        }

        return schemas;
    }

    private static async loadFromDir(dir: string, registry: Record<string, SchemaBuilder>): Promise<void> {
        try {
            const files = await fs.readdir(dir);
            const schemaFiles = files.filter((file: string) => file.endsWith('.schema.json'));

            for (const file of schemaFiles) {
                const schemaName = path.basename(file, '.schema.json');
                // Avoid redundant loading if already in registry
                if (!registry[schemaName]) {
                    registry[schemaName] = await this.loadSchema(schemaName);
                }
            }
        } catch {
            // Directory might not exist or be empty
        }
    }

    private static validateSchema(schema: RawSchema): void {
        if (!schema.name || !schema.description || !schema.properties) {
            throw new Error('Schema must have name, description, and properties');
        }
    }
}