// src/core/metadata/Metadata.ts

/**
 * Represents metadata information associated with a node
 * Key-value pairs describing the entity
 */
export type Metadata = Record<string, unknown>;

export interface MetadataEntry {
    key: string;
    value: string;
}

export interface MetadataAddition {
    nodeName: string;
    contents: Record<string, unknown>;
}

export interface MetadataDeletion {
    nodeName: string;
    keys: string[];
}

export interface MetadataResult {
    nodeName: string;
    addedMetadata: string[];
}