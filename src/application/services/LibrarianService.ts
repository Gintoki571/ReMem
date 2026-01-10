// src/application/services/LibrarianService.ts

import { promises as fs } from 'fs';
import path from 'path';
import { CONFIG } from '@config/index.js';

interface ModuleMetadata {
    name: string;
    description: string;
    keywords: string[];
}

/**
 * The LibrarianService analyzes context to suggest relevant memory modules.
 * This helps manage the "Cognitive Load" of the AI by only loading what's needed.
 */
export class LibrarianService {
    private moduleMetadata: ModuleMetadata[] = [];
    private initialized = false;

    /**
     * Loads metadata for all available modules.
     */
    async initialize(): Promise<void> {
        if (this.initialized) return;

        try {
            const modulesDir = CONFIG.PATHS.MODULES_DIR;
            const entries = await fs.readdir(modulesDir, { withFileTypes: true });

            for (const entry of entries) {
                if (entry.isDirectory()) {
                    const metadataPath = path.join(modulesDir, entry.name, 'module.json');
                    try {
                        const content = await fs.readFile(metadataPath, 'utf-8');
                        const metadata = JSON.parse(content) as ModuleMetadata;
                        this.moduleMetadata.push(metadata);
                    } catch (err) {
                        // Skip modules without metadata
                        console.error(`[Librarian] No metadata found for module: ${entry.name}`);
                    }
                }
            }
            this.initialized = true;
            console.error(`[Librarian] Initialized with ${this.moduleMetadata.length} modules.`);
        } catch (error) {
            console.error('[Librarian] Initialization failed:', error);
        }
    }

    /**
     * Suggests modules based on the provided text context.
     */
    async suggestModules(context: string): Promise<{ name: string; score: number; reason: string }[]> {
        if (!this.initialized) await this.initialize();

        const suggestions: { name: string; score: number; reason: string }[] = [];
        const lowerContext = context.toLowerCase();

        for (const meta of this.moduleMetadata) {
            let score = 0;
            const matchedKeywords: string[] = [];

            // Simple keyword matching
            for (const keyword of meta.keywords) {
                if (lowerContext.includes(keyword.toLowerCase())) {
                    score += 1;
                    matchedKeywords.push(keyword);
                }
            }

            if (score > 0) {
                suggestions.push({
                    name: meta.name,
                    score,
                    reason: `Matched keywords: ${matchedKeywords.slice(0, 3).join(', ')}${matchedKeywords.length > 3 ? '...' : ''}`
                });
            }
        }

        // Sort by score descending
        return suggestions.sort((a, b) => b.score - a.score);
    }
}

export const librarianService = new LibrarianService();
