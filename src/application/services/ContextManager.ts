import { eq, and, asc, not } from 'drizzle-orm';
import { getDatabase, schema } from '@infrastructure/database/index.js';
import { analyzer } from '@application/services/Analyzer.js';

const TOKEN_LIMIT = 4000; // Trigger summarization at ~4k tokens
const KEEP_RECENT = 10;   // Always keep the last 10 messages raw

export class ContextManager {
    private isOptimizing = false;
    
    /**
     * Add a message to the history and trigger optimization check
     */
    async addMessage(role: 'user' | 'assistant' | 'system', content: string): Promise<void> {
        const db = getDatabase();
        
        // 1. Insert Message
        await db.insert(schema.messages).values({
            role,
            content,
            tokenCount: content.length / 4, // Rough approximation
            isSummarized: false
        });

        // 2. Check and Optimize (Fire and forget, don't block)
        if (!this.isOptimizing) {
            this.optimizeContext().catch(err => console.error('[ContextManager] Optimization failed:', err));
        }
    }

    /**
     * Get the current effective context (Summary + Recent Messages)
     * This is what the Agent should see.
     */
    async getEffectiveContext(): Promise<{ summary: string, recentMessages: schema.Message[] }> {
        const db = getDatabase();

        // 1. Get Summary
        const summaryRecord = await db.query.globalState.findFirst({
            where: eq(schema.globalState.key, 'rolling_summary')
        });
        const summary = summaryRecord ? summaryRecord.value : '';

        // 2. Get Unsummarized Messages
        const recentMessages = await db.query.messages.findMany({
            where: eq(schema.messages.isSummarized, false),
            orderBy: asc(schema.messages.createdAt)
        });

        return { summary, recentMessages };
    }

    /**
     * Letta-style "Rolling Summary" Logic
     */
    private async optimizeContext(): Promise<void> {
        if (this.isOptimizing) return;
        this.isOptimizing = true;

        try {
            const db = getDatabase();

            // 1. Count unsummarized tokens
            const unsummarized = await db.query.messages.findMany({
                where: eq(schema.messages.isSummarized, false),
                orderBy: asc(schema.messages.createdAt)
            });

            const totalTokens = unsummarized.reduce((sum, msg) => sum + (msg.tokenCount || 0), 0);

            if (totalTokens < TOKEN_LIMIT) return;

            console.log(`[ContextManager] Token limit exceeded (${totalTokens} > ${TOKEN_LIMIT}). Summarizing...`);

            // 2. Select messages to summarize (All except the last KEEP_RECENT)
            if (unsummarized.length <= KEEP_RECENT) return; // Not enough messages to compress

            const messagesToSummarize = unsummarized.slice(0, unsummarized.length - KEEP_RECENT);
            const idsToMark = messagesToSummarize.map(m => m.id);

            // 3. Get current summary
            const summaryRecord = await db.query.globalState.findFirst({
                where: eq(schema.globalState.key, 'rolling_summary')
            });
            const currentSummary = summaryRecord ? summaryRecord.value : '';

            // 4. Generate new summary
            const newSummary = await analyzer.summarizeMessages(
                messagesToSummarize.map(m => ({ role: m.role, content: m.content })),
                currentSummary
            );

            // 5. Update DB Transaction
            await db.transaction(async (tx) => {
                // Update Summary
                await tx.insert(schema.globalState)
                    .values({ key: 'rolling_summary', value: newSummary })
                    .onConflictDoUpdate({ target: schema.globalState.key, set: { value: newSummary, updatedAt: new Date() } });

                // Mark messages as summarized
                for (const id of idsToMark) {
                    await tx.update(schema.messages)
                        .set({ isSummarized: true })
                        .where(eq(schema.messages.id, id));
                }
            });

            console.log(`[ContextManager] Context optimized. Summary updated.`);
        } finally {
            this.isOptimizing = false;
        }
    }

    /**
     * Manual force summary (useful for tools)
     */
    async forceSummarize(): Promise<string> {
        await this.optimizeContext();
        const { summary } = await this.getEffectiveContext();
        return summary;
    }
}

export const contextManager = new ContextManager();
