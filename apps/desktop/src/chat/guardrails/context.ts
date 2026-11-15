import type { SessionContext } from "@anlg/plugin-template";

import { contentWords, extractMeetingTerms } from "./classifier";
import { type AskScope, askThreadKey, isAskScope } from "./scope";

export type LibraryExcerpt = {
  meetingId: string;
  title: string;
  date: string | null;
  excerpt: string;
};

export type AskContextDeps = {
  hydrateMeeting: (meetingId: string) => Promise<SessionContext | null>;
  searchLibrary: (query: string, limit: number) => Promise<LibraryExcerpt[]>;
};

export type AssembledAskContext = {
  scope: AskScope;
  meetingId: string | null;
  /** The `<meeting_sources>` block prepended to the current message, if any. */
  sourcesBlock: string | null;
  /** Every meeting whose content appears in `sourcesBlock`. */
  includedMeetingIds: string[];
  /** Library scope only: the search found nothing. */
  emptyLibrary: boolean;
  /** Meeting scope: the meeting could not be loaded. */
  meetingMissing: boolean;
};

export const LIBRARY_TOP_K = 5;
const LIBRARY_EXCERPT_MAX_CHARS = 320;

/** HH:MM:SS, so citations like [00:04:12] are unambiguous. */
export function formatTimestamp(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return [hours, minutes, seconds]
    .map((value) => String(value).padStart(2, "0"))
    .join(":");
}

export function formatMeetingDate(date: string | null | undefined): string {
  if (!date) return "unknown date";
  const iso = date.match(/^\d{4}-\d{2}-\d{2}/);
  if (iso) return iso[0];
  const parsed = new Date(date);
  return Number.isNaN(parsed.getTime())
    ? date
    : parsed.toISOString().slice(0, 10);
}

// Source text can never close the data block early.
function escapeSourceText(text: string): string {
  return text.replace(/<\s*\/?\s*meeting_sources/gi, (match) =>
    match.replace("<", "&lt;"),
  );
}

export function renderMeetingSources(ctx: SessionContext): string {
  const lines: string[] = [`<meeting_sources scope="meeting">`];
  lines.push(`Meeting: ${escapeSourceText(ctx.title?.trim() || "Untitled")}`);
  if (ctx.sessionId) lines.push(`Meeting ID: ${ctx.sessionId}`);
  lines.push(`Date: ${formatMeetingDate(ctx.date)}`);
  if (ctx.participants.length > 0) {
    lines.push(
      `Participants: ${ctx.participants
        .map((p) =>
          escapeSourceText(p.jobTitle ? `${p.name} (${p.jobTitle})` : p.name),
        )
        .join(", ")}`,
    );
  }

  const segments = ctx.transcript?.segments ?? [];
  lines.push("", "## Transcript");
  if (segments.length === 0) {
    lines.push("(no transcript)");
  } else {
    for (const segment of segments) {
      const stamp =
        typeof segment.startMs === "number"
          ? `[${formatTimestamp(segment.startMs)}] `
          : "";
      lines.push(
        `${stamp}${escapeSourceText(segment.speaker)}: ${escapeSourceText(segment.text)}`,
      );
    }
    if (typeof ctx.transcript?.nowMs === "number") {
      lines.push(
        `[${formatTimestamp(ctx.transcript.nowMs)}] (now, still recording)`,
      );
    }
  }

  if (ctx.rawContent?.trim()) {
    lines.push("", "## User notes", escapeSourceText(ctx.rawContent.trim()));
  }
  if (ctx.enhancedContent?.trim()) {
    lines.push(
      "",
      "## Summary / minutes",
      escapeSourceText(ctx.enhancedContent.trim()),
    );
  }
  if (ctx.meetingChat?.trim()) {
    lines.push("", "## Meeting chat", escapeSourceText(ctx.meetingChat.trim()));
  }
  lines.push("</meeting_sources>");
  return lines.join("\n");
}

export function renderLibrarySources(excerpts: LibraryExcerpt[]): string {
  if (excerpts.length === 0) {
    return [
      `<meeting_sources scope="library">`,
      "No matching meeting excerpts were found.",
      "</meeting_sources>",
    ].join("\n");
  }
  const lines = [`<meeting_sources scope="library">`];
  excerpts.forEach((item, index) => {
    const excerpt = item.excerpt.replace(/\s+/g, " ").trim();
    lines.push(
      `[${index + 1}] Meeting: ${escapeSourceText(item.title.trim() || "Untitled")} | Date: ${formatMeetingDate(item.date)} | ID: ${item.meetingId}`,
      `Excerpt: ${escapeSourceText(
        excerpt.length > LIBRARY_EXCERPT_MAX_CHARS
          ? `${excerpt.slice(0, LIBRARY_EXCERPT_MAX_CHARS)}…`
          : excerpt,
      )}`,
    );
  });
  lines.push("</meeting_sources>");
  return lines.join("\n");
}

/** Content-word query for the full-text index ("what did we decide…" → "decide …"). */
export function buildLibrarySearchQuery(query: string): string {
  const words = contentWords(query);
  return (words.length > 0 ? words : [query.trim()]).join(" ");
}

/** Local word set for classifier overlap; never sent to a model. */
export function meetingTermsFromContext(ctx: SessionContext | null) {
  if (!ctx) return new Set<string>();
  return extractMeetingTerms([
    ctx.title,
    ...ctx.participants.map((p) => p.name),
    ...(ctx.transcript?.segments ?? []).map((s) => s.text),
    ctx.rawContent,
    ctx.enhancedContent,
    ctx.event?.name,
  ]);
}

/** Top-k library search; failures yield no excerpts. */
export async function searchLibrarySafely(
  query: string,
  deps: Pick<AskContextDeps, "searchLibrary">,
): Promise<LibraryExcerpt[]> {
  try {
    return (
      await deps.searchLibrary(buildLibrarySearchQuery(query), LIBRARY_TOP_K)
    ).slice(0, LIBRARY_TOP_K);
  } catch (error) {
    console.error("[ask-guardrails] library search failed", error);
    return [];
  }
}

/**
 * Builds the only meeting context a model sees for one Ask request.
 * - meeting: that one meeting (transcript with timestamps, notes, summary).
 * - library: top-k search excerpts labelled with meeting title, date, id.
 * Refs from earlier messages are never consulted.
 */
export async function assembleAskContext(
  {
    scope,
    meetingId,
    query,
    meeting,
    excerpts: prefetched,
  }: {
    scope: AskScope;
    meetingId?: string | null;
    query: string;
    /** Pre-hydrated meeting (the transport loads it once for the classifier). */
    meeting?: SessionContext | null;
    /** Pre-fetched library excerpts (the transport searches before classifying). */
    excerpts?: LibraryExcerpt[];
  },
  deps: AskContextDeps,
): Promise<AssembledAskContext> {
  const base = {
    scope,
    meetingId: scope === "meeting" ? (meetingId ?? null) : null,
    emptyLibrary: false,
    meetingMissing: false,
  };

  if (scope === "meeting") {
    if (!meetingId) {
      return {
        ...base,
        sourcesBlock: null,
        includedMeetingIds: [],
        meetingMissing: true,
      };
    }
    const ctx =
      meeting !== undefined ? meeting : await deps.hydrateMeeting(meetingId);
    // Only the requested meeting may ever be rendered.
    if (!ctx || (ctx.sessionId && ctx.sessionId !== meetingId)) {
      return {
        ...base,
        sourcesBlock: null,
        includedMeetingIds: [],
        meetingMissing: true,
      };
    }
    return {
      ...base,
      sourcesBlock: renderMeetingSources({ ...ctx, sessionId: meetingId }),
      includedMeetingIds: [meetingId],
    };
  }

  const excerpts = (
    prefetched ?? (await searchLibrarySafely(query, deps))
  ).slice(0, LIBRARY_TOP_K);
  return {
    ...base,
    sourcesBlock: renderLibrarySources(excerpts),
    includedMeetingIds: [...new Set(excerpts.map((item) => item.meetingId))],
    emptyLibrary: excerpts.length === 0,
  };
}

type HistoryMessage = { role: string; metadata?: unknown };

export function getMessageAskThreadKey(metadata: unknown): string | null {
  if (typeof metadata !== "object" || metadata === null) return null;
  const record = metadata as Record<string, unknown>;
  if (!isAskScope(record.askScope)) return null;
  return askThreadKey(
    record.askScope,
    typeof record.askMeetingId === "string" ? record.askMeetingId : undefined,
  );
}

/**
 * History rule: send only the current message. When it refers to a prior
 * output, add just the most recent assistant turn of the same thread key —
 * never older turns, never other keys.
 */
export function selectHistoryForModel<T extends HistoryMessage>(
  messages: T[],
  { threadKey, refersToPrior }: { threadKey: string; refersToPrior: boolean },
): T[] {
  let lastUserIndex = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]?.role === "user") {
      lastUserIndex = i;
      break;
    }
  }
  if (lastUserIndex === -1) return [];
  const current = messages[lastUserIndex]!;
  if (!refersToPrior) return [current];

  let turnKey: string | null = null;
  let lastAssistant: T | undefined;
  for (const message of messages.slice(0, lastUserIndex)) {
    if (message.role === "user") {
      turnKey = getMessageAskThreadKey(message.metadata);
    } else if (message.role === "assistant" && turnKey === threadKey) {
      lastAssistant = message;
    }
  }
  return lastAssistant ? [lastAssistant, current] : [current];
}

/** Keeps only turns whose user message belongs to `threadKey`. */
export function filterMessagesForAskThread<T extends HistoryMessage>(
  messages: T[],
  threadKey: string,
): T[] {
  const kept: T[] = [];
  let include = false;
  for (const message of messages) {
    if (message.role === "user") {
      include = getMessageAskThreadKey(message.metadata) === threadKey;
    }
    if (include) kept.push(message);
  }
  return kept;
}
