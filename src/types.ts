import { z } from "zod";

export const MEMORY_KINDS = ["fact", "decision", "mistake", "preference", "event", "note"] as const;

export const MemoryKindSchema = z.enum(MEMORY_KINDS);
export type MemoryKind = z.infer<typeof MemoryKindSchema>;

export const MemoryItemSchema = z.object({
  id: z.string(),
  kind: MemoryKindSchema,
  content: z.string().min(1),
  tags: z.array(z.string()).default([]),
  source: z.string().default("agent"),
  sessionId: z.string().optional(),
  agentId: z.string().optional(),
  importance: z.number().min(0).max(1).default(0.5),
  createdAt: z.number().int(),
  updatedAt: z.number().int(),
  validFrom: z.number().int(),
  validTo: z.number().int().nullish(),
});

export type MemoryItem = z.infer<typeof MemoryItemSchema>;

export type MemoryItemInput = Omit<MemoryItem, "id" | "createdAt" | "updatedAt" | "validFrom"> & {
  id?: string;
  createdAt?: number;
  updatedAt?: number;
  validFrom?: number;
};

export const RecallQuerySchema = z.object({
  text: z.string().default(""),
  k: z.number().int().positive().default(10),
  kinds: z.array(MemoryKindSchema).optional(),
  tags: z.array(z.string()).optional(),
  agentId: z.string().optional(),
  sessionId: z.string().optional(),
  includeDeleted: z.boolean().default(false),
});

export type RecallQuery = z.infer<typeof RecallQuerySchema>;

export type RecallHit = {
  item: MemoryItem;
  score: number;
  reasons: string[];
};
