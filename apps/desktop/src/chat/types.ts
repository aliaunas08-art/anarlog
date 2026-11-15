import type { UIMessage } from "ai";
import { z } from "zod";

import { CONTEXT_ENTITY_SOURCES } from "~/chat/context/entities";
import type { ContextRef } from "~/chat/context/entities";
import { ASK_SCOPES } from "~/chat/guardrails/scope";

export type ChatScope = "general" | "automations";

const messageMetadataSchema = z.object({
  chatScope: z.enum(["general", "automations"]).optional(),
  createdAt: z.number().optional(),
  // Ask thread key parts. Optional: messages from before scoped threads have
  // neither and are never loaded into a keyed thread.
  askScope: z.enum(ASK_SCOPES).optional(),
  askMeetingId: z.string().optional(),
  // Assistant only: a one-line redirect with buttons to re-ask elsewhere.
  askRedirect: z
    .object({
      query: z.string(),
      actions: z.array(z.enum(["library", "general"])),
    })
    .optional(),
  // Assistant only: General-scope answers show the general-knowledge badge.
  askBadge: z.literal("general").optional(),
  askNotice: z.literal("usage_limit").optional(),
  contextRefs: z
    .array(
      z.discriminatedUnion("kind", [
        z.object({
          kind: z.literal("session"),
          key: z.string(),
          source: z.enum(CONTEXT_ENTITY_SOURCES).optional(),
          sessionId: z.string(),
        }),
        z.object({
          kind: z.literal("human"),
          key: z.string(),
          source: z.enum(CONTEXT_ENTITY_SOURCES).optional(),
          humanId: z.string(),
        }),
        z.object({
          kind: z.literal("organization"),
          key: z.string(),
          source: z.enum(CONTEXT_ENTITY_SOURCES).optional(),
          organizationId: z.string(),
        }),
        z.object({
          kind: z.literal("folder"),
          key: z.string(),
          source: z.enum(CONTEXT_ENTITY_SOURCES).optional(),
          folderId: z.string(),
        }),
      ]),
    )
    .optional(),
});

type MessageMetadata = z.infer<typeof messageMetadataSchema>;
export type AnlgUIMessage = UIMessage<
  MessageMetadata & { contextRefs?: ContextRef[] }
>;

export type ChatSendOptions = {
  chatGroupId?: string;
  beforeSend?: (
    trackCompletion: (completion: Promise<unknown>) => void,
  ) => void | Promise<void>;
};

export type ChatMessageSender = (
  message: AnlgUIMessage,
  options?: ChatSendOptions,
) => void;
