import { formatToolError } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import type { ToolResponse } from '@shared/index.js';
import { z } from 'zod';

export abstract class BaseToolHandler {
    constructor(protected knowledgeGraphManager: ApplicationManager) {
    }

    abstract handleTool(name: string, args: Record<string, any>): Promise<ToolResponse>;

    protected validateArguments(args: Record<string, any>): void {
        if (!args) {
            throw new Error("Tool arguments are required");
        }
    }

    /**
     * Helper to validate arguments against a Zod schema
     */
    protected validateSchema<T>(schema: z.ZodSchema<T>, args: unknown): T {
        const result = schema.safeParse(args);
        if (!result.success) {
            const errorMessages = result.error.errors.map(e => `${e.path.join('.')}: ${e.message}`).join('; ');
            throw new Error(`Validation failed: ${errorMessages}`);
        }
        return result.data;
    }

    /**
     * Helper to validate that specific arguments exist
     */
    protected validateRequiredArgs(args: Record<string, any>, required: string[]): void {
        this.validateArguments(args);
        const missing = required.filter(arg => args[arg] === undefined || args[arg] === null);
        if (missing.length > 0) {
            throw new Error(`Missing required arguments: ${missing.join(', ')}`);
        }
    }

    protected handleError(name: string, error: unknown): ToolResponse {
        console.error(`Error in ${name}:`, error);
        return formatToolError({
            operation: name,
            error: error instanceof Error ? error.message : 'Unknown error occurred',
            context: { toolName: name },
            suggestions: ["Examine the tool input parameters for correctness.", "Verify that the requested operation is supported."],
            recoverySteps: ["Adjust the input parameters based on the schema definition."]
        });
    }
}