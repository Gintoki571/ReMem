// src/core/metadata/MetadataProcessor.ts

import { MetadataEntry, Metadata } from './Metadata.js';

export class MetadataProcessor {
    /**
     * Formats a metadata entry into a string
     */
    static formatMetadataEntry(key: string, value: string | string[] | unknown): string {
        if (Array.isArray(value)) {
            return `${key}: ${value.join(', ')}`;
        }
        return `${key}: ${String(value)}`;
    }

    /**
     * Processes and validates metadata entries
     */
    static validateMetadata(metadata: Metadata): boolean {
        return typeof metadata === 'object' && metadata !== null && !Array.isArray(metadata);
    }

    /**
     * Merges multiple metadata objects
     */
    static mergeMetadata(...metadataObjects: Metadata[]): Metadata {
        return Object.assign({}, ...metadataObjects);
    }

    /**
     * Extracts value for a specific metadata key
     */
    static getValue(metadata: Metadata, key: string): unknown | null {
        return metadata[key] ?? null;
    }

    /**
     * Creates a metadata entry map (Redundant but kept for compatibility)
     */
    static createMetadataMap(metadata: Metadata): Map<string, string> {
        const map = new Map<string, string>();
        Object.entries(metadata).forEach(([k, v]) => {
            map.set(k, String(v));
        });
        return map;
    }
}