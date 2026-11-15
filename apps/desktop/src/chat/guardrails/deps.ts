import { generateText, type LanguageModel } from "ai";

import { commands as tantivy } from "@anlg/plugin-tantivy";

import {
  ASK_CLASSIFIER_PROMPT,
  type AskModelClassifier,
  parseClassifierReply,
} from "./classifier";
import type { LibraryExcerpt } from "./context";

import { liveQueryClient } from "~/db";
import { normalizeQuery } from "~/search/contexts/engine/utils";

const CLASSIFIER_TIMEOUT_MS = 4000;

/** Top-k library excerpts from the full-text index, using its snippets. */
export async function searchLibraryExcerpts(
  query: string,
  limit: number,
): Promise<LibraryExcerpt[]> {
  const normalized = normalizeQuery(query);
  if (!normalized) return [];
  const result = await tantivy.search({
    query: normalized,
    filters: { created_at: null, doc_type: "session", facet: null },
    limit: limit * 2,
    options: {
      fuzzy: null,
      distance: null,
      snippets: true,
      snippet_max_chars: 280,
      phrase_slop: null,
    },
  });
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data.hits
    .filter((hit) => hit.document.doc_type === "session")
    .slice(0, limit)
    .map((hit) => ({
      meetingId: hit.document.id,
      title: hit.document.title,
      date:
        Number.isFinite(hit.document.created_at) && hit.document.created_at > 0
          ? new Date(hit.document.created_at).toISOString()
          : null,
      excerpt:
        hit.content_snippet?.fragment?.trim() ||
        hit.document.content.slice(0, 280),
    }));
}

export async function listMeetingTitles(): Promise<
  Array<{ id: string; title: string }>
> {
  const rows = await liveQueryClient.execute<{ id: string; title: string }>(
    `
      SELECT id, title
      FROM sessions
      WHERE deleted_at IS NULL AND title IS NOT NULL AND trim(title) != ''
      ORDER BY created_at DESC
      LIMIT 500
    `,
    [],
  );
  return rows;
}

/**
 * Cheap fallback classifier: the query alone (no transcript), a one-word
 * reply, and a short timeout. Callers treat any failure as "answer".
 */
export function createModelClassifier(
  model: LanguageModel | null,
): AskModelClassifier | undefined {
  if (!model) return undefined;
  return async ({ query, scope, meetingTitle }) => {
    const { text } = await generateText({
      model,
      system: ASK_CLASSIFIER_PROMPT,
      prompt: [
        `Scope: ${scope === "meeting" ? "one meeting" : "all meetings"}`,
        meetingTitle ? `Meeting title: ${meetingTitle}` : null,
        `Request: ${query.slice(0, 500)}`,
      ]
        .filter(Boolean)
        .join("\n"),
      maxOutputTokens: 8,
      abortSignal: AbortSignal.timeout(CLASSIFIER_TIMEOUT_MS),
    });
    return parseClassifierReply(text);
  };
}
