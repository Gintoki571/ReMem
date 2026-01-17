
import { getDatabase, schema } from '@infrastructure/database/index.js';
import { CONFIG } from '@config/config.js';
import { analyzer } from '@application/services/Analyzer.js';
import { eq } from 'drizzle-orm';
import { TokenEstimator } from '@core/tokenizer/TokenEstimator.js';
import { Logger } from '@core/logging/Logger.js';

export interface ContextTiers {
    l1_system: string;
    l2_summary: string;
    l3_conversation: string;
}


export class ContextManager {
    // Track message counts per user for auto-compaction
    private messageCounter = new Map<string, number>();

    constructor() {
    }

    public async getEffectiveContext(userId: string = 'default'): Promise<ContextTiers> {
        console.error('[ContextManager] Getting L1 context...');
        const l1 = await this.getL1Context();

        console.error('[ContextManager] Getting L2 context...');
        const l2 = await this.getL2Context(userId);

        console.error('[ContextManager] Getting L3 context...');
        const l3 = await this.getL3Context(userId);

        return {
            l1_system: l1,
            l2_summary: l2,
            l3_conversation: l3
        };
    }

    private async getL1Context(): Promise<string> {
        // Fetch global scope nodes from Graph
        const db = getDatabase();

        const globalNodes = await db.query.nodes.findMany({
            where: (nodes, { eq }) => eq(nodes.nodeType, 'global_fact')
        });

        const globalFacts = globalNodes
            .map(node => {
                if (!node.metadata) return null;
                if (!node.metadata) return null;
                try {
                    // Drizzle with mode: 'json' returns the object directly
                    const meta = node.metadata as Record<string, any>;

                    if (meta.content) {
                        return `- ${meta.content}`;
                    }
                    return null;
                } catch (e) {
                    return null;
                }
            })
            .filter(f => f !== null)
            .join('\n');

        const baseSystem = `
[SYSTEM PROFILE]
You are ReMem, an intelligent coding assistant with a "Second Brain".
You have access to a persistent Knowledge Graph to store and retrieve facts.
`;

        if (globalFacts) {
            return `${baseSystem}\n[GLOBAL FACTS]\n${globalFacts}\n`;
        }
        return baseSystem;
    }

    private async getL2Context(userId: string): Promise<string> {
        const db = getDatabase();
        // Fetch the latest summary from 'global_state' or dedicated table
        // For MVP, we return a placeholder or look for a specifically named node
        const summaryNode = await db.query.nodes.findFirst({
            where: (nodes, { eq }) => eq(nodes.name, `${userId}_summary`)
        });

        if (summaryNode && summaryNode.metadata) {
            const meta = summaryNode.metadata as Record<string, any>;
            return meta.content || "";
        }
        return "";
    }

    private async getL3Context(userId: string): Promise<string> {
        // Retrieve last N messages from ConversationManager (managed via DB)
        const db = getDatabase();
        // Fetch more messages than needed (e.g. 50) and filter by token budget
        const recentMessages = await db.query.messages.findMany({
            orderBy: (messages, { desc }) => [desc(messages.createdAt)],
            limit: 50,
        });

        // Loop backwards to build context up to limit
        const contextMessages: string[] = [];
        let currentTokens = 0;
        const MAX_L3_TOKENS = CONFIG.CONTEXT.MAX_L3_TOKENS;

        // Iterate from most recent backwards
        for (const msg of recentMessages) {
            const formattedMsg = `${msg.role.toUpperCase()}: ${msg.content}`;
            const tokens = TokenEstimator.countTokens(formattedMsg);

            if (currentTokens + tokens > MAX_L3_TOKENS) {
                break;
            }

            contextMessages.push(formattedMsg);
            currentTokens += tokens;
        }

        console.error(`[ContextManager] L3 Context built: ${contextMessages.length} msgs, ~${currentTokens} tokens`);

        // Reverse to chronological order (Oldest -> Newest)
        return contextMessages.reverse().join('\n');
    }

    /**
     * Updates the Rolling Summary (L2) by compacting older L3 messages.
     * This should be called periodically (e.g., via a background job or after N turns).
     */
    public async compactContext(userId: string): Promise<void> {
        try {
            const db = getDatabase();

            // 1. Fetch unsummarized messages
            const unsummarizedMessages = await db.query.messages.findMany({
                where: (messages, { eq }) => eq(messages.isSummarized, false),
                orderBy: (messages, { asc }) => [asc(messages.createdAt)],
            });

            if (unsummarizedMessages.length === 0) {
                console.error('[ContextManager] No messages to compact');
                return;
            }

            // 2. Get current summary
            const summaryNode = await db.query.nodes.findFirst({
                where: (nodes, { eq }) => eq(nodes.name, `${userId}_summary`)
            });

            let currentSummary = '';
            if (summaryNode && summaryNode.metadata) {
                const meta = summaryNode.metadata as Record<string, any>;
                currentSummary = meta.content || '';
            }

            // 3. Generate new summary using LLM
            const messagesToSummarize = unsummarizedMessages.map(m => ({
                role: m.role,
                content: m.content
            }));

            const newSummary = await analyzer.summarizeMessages(messagesToSummarize, currentSummary);

            try {
                // 4. Update L2 state (create or update summary node)
                if (summaryNode) {
                    // Update existing summary
                    db.update(schema.nodes)
                        .set({
                            metadata: { content: newSummary },
                            updatedAt: new Date()
                        })
                        .where(eq(schema.nodes.name, `${userId}_summary`))
                        .run();
                } else {
                    // Create new summary node
                    db.insert(schema.nodes).values({
                        name: `${userId}_summary`,
                        nodeType: 'context_summary',
                        metadata: { content: newSummary },
                    }).run();
                }

                // 5. Mark messages as summarized
                for (const msg of unsummarizedMessages) {
                    db.update(schema.messages)
                        .set({ isSummarized: true })
                        .where(eq(schema.messages.id, msg.id))
                        .run();
                }
            } catch (dbError) {
                console.error('[ContextManager] Database operation failed during compaction:', dbError);
                throw dbError;
            }

            Logger.info('ContextManager', `Compacted ${unsummarizedMessages.length} messages into L2 summary`);
        } catch (error) {
            Logger.error('ContextManager', `Context compaction failed: ${error instanceof Error ? error.message : 'Unknown'}`);
            throw new Error(`Context compaction failed: ${error instanceof Error ? error.message : 'Unknown error'}`);
        }
    }

    /**
     * Log a message and check if auto-compaction should trigger.
     * Call this after storing each user/assistant message.
     */
    public async logMessageAndCheckCompaction(userId: string = 'default'): Promise<void> {
        const count = (this.messageCounter.get(userId) || 0) + 1;
        this.messageCounter.set(userId, count);

        Logger.debug('ContextManager', `Message count for ${userId}: ${count}/${CONFIG.CONTEXT.COMPACTION_THRESHOLD}`);

        if (count >= CONFIG.CONTEXT.COMPACTION_THRESHOLD) {
            Logger.info('ContextManager', `Threshold reached for ${userId}, triggering auto-compaction`);
            try {
                await this.compactContext(userId);
                this.messageCounter.set(userId, 0); // Reset after successful compaction
            } catch (error) {
                Logger.error('ContextManager', `Auto-compaction failed: ${error instanceof Error ? error.message : 'Unknown'}`);
                // Don't reset counter on failure - will retry next time
            }
        }
    }

    /**
     * Get current message count for a user (for debugging/monitoring)
     */
    public getMessageCount(userId: string = 'default'): number {
        return this.messageCounter.get(userId) || 0;
    }
}
