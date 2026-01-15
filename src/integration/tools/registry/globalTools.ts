
import type { Tool } from '@shared/index.js';

export const globalTools: Tool[] = [
    {
        name: "add_global_memory",
        description: "Add a core fact or user preference that should ALWAYS be remembered, regardless of the active module. Use this for user bio, coding style preferences, and critical project constraints.",
        inputSchema: {
            type: "object",
            properties: {
                facts: {
                    type: "array",
                    description: "List of facts to store globally",
                    items: {
                        type: "string",
                        description: "The fact content (e.g., 'User prefers dark mode', 'User is a senior engineer')"
                    }
                }
            },
            required: ["facts"]
        }
    }
];
