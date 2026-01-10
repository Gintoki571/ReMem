// src/integration/tools/handlers/ModuleHandler.ts

import { BaseToolHandler } from './BaseToolHandler.js';
import { CONFIG } from '@config/index.js';
import { formatToolResponse, formatToolError } from '@shared/index.js';
import type { Tool, ToolResponse } from '@shared/index.js';
import { dynamicSchemaTools } from '../DynamicSchemaToolRegistry.js';
import { toolsRegistry } from '../registry/toolsRegistry.js';
import { promises as fs } from 'fs';
import path from 'path';

export const moduleTools: Tool[] = [
    {
        name: "list_modules",
        description: "List all available memory modules and their current status (Active/Inactive).",
        inputSchema: {
            type: "object",
            properties: {}
        }
    },
    {
        name: "activate_module",
        description: "Activate a specific memory module (e.g., 'coding', 'rpg') to enable its specialized tools.",
        inputSchema: {
            type: "object",
            properties: {
                moduleName: { type: "string", description: "The name of the module to activate" }
            },
            required: ["moduleName"]
        }
    },
    {
        name: "deactivate_module",
        description: "Deactivate a memory module to clean up the toolset and save context window space.",
        inputSchema: {
            type: "object",
            properties: {
                moduleName: { type: "string", description: "The name of the module to deactivate" }
            },
            required: ["moduleName"]
        }
    },
    {
        name: "librarian_suggest",
        description: "Analyze the current context and suggest which memory modules should be activated.",
        inputSchema: {
            type: "object",
            properties: {
                context: { type: "string", description: "The current project context or user prompt to analyze" }
            },
            required: ["context"]
        }
    }
];

export class ModuleHandler extends BaseToolHandler {
    async handleTool(toolName: string, args: any): Promise<ToolResponse> {
        switch (toolName) {
            case 'list_modules':
                return this.handleListModules();
            case 'activate_module':
                return this.handleActivateModule(args.moduleName);
            case 'deactivate_module':
                return this.handleDeactivateModule(args.moduleName);
            case 'librarian_suggest':
                return this.handleLibrarianSuggest(args.context);
            default:
                throw new Error(`Tool not handled by ModuleHandler: ${toolName}`);
        }
    }

    private async handleListModules(): Promise<ToolResponse> {
        try {
            const modulesPath = CONFIG.PATHS.MODULES_DIR;
            const entries = await fs.readdir(modulesPath, { withFileTypes: true });
            const availableModules = entries
                .filter((e: any) => e.isDirectory())
                .map((e: any) => e.name);

            const status = availableModules.map((m: string) => ({
                name: m,
                status: CONFIG.MODULES.ACTIVE.includes(m) ? "ACTIVE" : "INACTIVE"
            }));

            return formatToolResponse({
                data: { modules: status },
                actionTaken: "Listed available modules"
            });
        } catch (error) {
            return formatToolError({
                operation: "list_modules",
                error: "Failed to list modules",
                suggestions: ["Check if modules directory exists"]
            });
        }
    }

    private async handleActivateModule(moduleName: string): Promise<ToolResponse> {
        if (CONFIG.MODULES.ACTIVE.includes(moduleName)) {
            return formatToolResponse({
                data: { moduleName, status: "ALREADY_ACTIVE" },
                actionTaken: `Module ${moduleName} is already active`
            });
        }

        // Verify module exists
        const modulePath = path.join(CONFIG.PATHS.MODULES_DIR, moduleName);
        try {
            await fs.access(modulePath);
        } catch {
            return formatToolError({
                operation: "activate_module",
                error: `Module '${moduleName}' not found`,
                suggestions: ["Verify module name", "Call list_modules to see available options"]
            });
        }

        // Activate
        CONFIG.MODULES.ACTIVE.push(moduleName);

        // Refresh dynamic tools
        await dynamicSchemaTools.refresh();
        await toolsRegistry.refresh();

        return formatToolResponse({
            data: { moduleName, status: "ACTIVATED" },
            actionTaken: `Activated module: ${moduleName}. New tools are now available.`
        });
    }

    private async handleDeactivateModule(moduleName: string): Promise<ToolResponse> {
        const index = CONFIG.MODULES.ACTIVE.indexOf(moduleName);
        if (index === -1) {
            return formatToolResponse({
                data: { moduleName, status: "NOT_ACTIVE" },
                actionTaken: `Module ${moduleName} is not active`
            });
        }

        // Deactivate
        CONFIG.MODULES.ACTIVE.splice(index, 1);

        // Refresh dynamic tools
        await dynamicSchemaTools.refresh();
        await toolsRegistry.refresh();

        return formatToolResponse({
            data: { moduleName, status: "DEACTIVATED" },
            actionTaken: `Deactivated module: ${moduleName}. Context window cleared.`
        });
    }

    private async handleLibrarianSuggest(context: string): Promise<ToolResponse> {
        const { librarianService } = await import('../../../application/services/LibrarianService.js');
        const suggestions = await librarianService.suggestModules(context);

        if (suggestions.length === 0) {
            return formatToolResponse({
                data: { suggestions: [] },
                actionTaken: "The Librarian found no specific module matches for this context. Stick with Core tools."
            });
        }

        const topModule = suggestions[0].name;
        const isActive = CONFIG.MODULES.ACTIVE.includes(topModule);

        return formatToolResponse({
            data: {
                suggestions,
                recommendation: isActive ? `Maintain ${topModule} (already active)` : `Activate ${topModule}`
            },
            actionTaken: `The Librarian suggests activating the '${topModule}' module based on matched keywords.`
        });
    }
}
