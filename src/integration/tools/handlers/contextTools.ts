import type { Tool, ToolResponse } from '@shared/index.js';
import type { ApplicationManager } from '@application/index.js';
import { contextManager } from '@application/services/ContextManager.js';

/**
 * log_interaction - Manually log a message to the context history
 */
export const logInteractionTool: Tool = {
    name: 'log_interaction',
    description: 'Log a user or assistant message to the Rolling Context. Use this to keep track of the conversation history so ReMem can summarize it.',
    inputSchema: {
        type: 'object',
        properties: {
            role: {
                type: 'string',
                enum: ['user', 'assistant', 'system'],
                description: 'The speaker role',
            },
            content: {
                type: 'string',
                description: 'The message content',
            },
        },
        required: ['role', 'content'],
    },
};

/**
 * get_context - Retrieve the rolling summary and recent messages
 */
export const getContextTool: Tool = {
    name: 'get_context',
    description: 'Get the current "Effective Context" (Rolling Summary + Recent Messages). Use this to "remember" what happened in the conversation if you have forgotten.',
    inputSchema: {
        type: 'object',
        properties: {},
    },
};

export async function handleLogInteraction(
    args: { role: 'user' | 'assistant' | 'system'; content: string },
    _manager: ApplicationManager
): Promise<ToolResponse> {
    await contextManager.addMessage(args.role, args.content);
    return {
        toolResult: {
            isError: false,
            data: { status: 'logged' },
            actionTaken: 'Logged message to context',
            timestamp: new Date().toISOString(),
            content: [{ type: 'text', text: 'Message logged.' }],
        }
    };
}

export async function handleGetContext(
    _args: {},
    _manager: ApplicationManager
): Promise<ToolResponse> {
    const { summary, recentMessages } = await contextManager.getEffectiveContext();
    
    const formattedRecent = recentMessages.map(m => `[${m.role.toUpperCase()}]: ${m.content}`).join('\n\n');
    const output = `## Rolling Summary (Long Term Context)\n${summary || 'No summary yet.'}\n\n## Recent Conversation (Short Term Memory)\n${formattedRecent}`;

    return {
        toolResult: {
            isError: false,
            data: { summary, recentMessages },
            actionTaken: 'Retrieved context',
            timestamp: new Date().toISOString(),
            content: [{ type: 'text', text: output }],
        }
    };
}

export const contextTools = [logInteractionTool, getContextTool];
