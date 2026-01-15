
/**
 * Centralized Prompt Configuration
 * Allows for easy tuning of LLM prompts without recompiling the application.
 */
export const PROMPTS = {
    EXTRACTION: {
        SYSTEM: `Analyze the following text and extract entities and relationships.

Output ONLY valid JSON matching this structure:
{
  "entities": [
    { "name": "string", "nodeType": "string", "metadata": ["string"] }
  ],
  "relationships": [
    { "from": "string", "to": "string", "edgeType": "string" }
  ]
}

Common types: npc, location, artifact, quest, faction, player_character, currency, transportation.
Common relationship types: located_in, owns, member_of, allied_with, enemy_of, knows, related_to, part_of, started_by, completed_by.`,

        GLOBAL_CONTEXT_TEMPLATE: (userBio: string) => `
Global Context (Who the User Is):
"${userBio}"
Use this context to resolve "I", "me", "my" references in the text.
`,
    },

    SMART_MERGE: {
        SYSTEM: `You are a smart memory manager.
Compare newly retrieved facts with existing memories.

Decide for EACH new fact:
- ADD: New info not present.
- UPDATE: Info exists but is different/more detailed. (Reuse ID)
- DELETE: Info contradicts memory. (Reuse ID)
- NONE: Info is already there.

Return JSON array:
[
  { "action": "ADD", "text": "..." },
  { "action": "UPDATE", "id": "...", "text": "..." },
  ...
]`,
    },

    SUMMARIZATION: {
        SYSTEM: `Your job is to summarize a history of previous messages in a conversation between an AI assistant and a human.
Output ONLY the new summary text (under 200 words). Maintain narrative flow.`
    }
};
