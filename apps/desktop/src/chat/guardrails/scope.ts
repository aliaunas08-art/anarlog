// Ask chat scopes. A thread belongs to exactly one scope (and, for "meeting",
// one meeting); switching either starts or loads that key's own thread.
export const ASK_SCOPES = ["meeting", "library", "general"] as const;
export type AskScope = (typeof ASK_SCOPES)[number];

export type AskThreadKey = string;

export function askThreadKey(
  scope: AskScope,
  meetingId: string | undefined,
): AskThreadKey {
  if (scope === "meeting") {
    return meetingId ? `meeting:${meetingId}` : "meeting:none";
  }
  return scope;
}

export function isAskScope(value: unknown): value is AskScope {
  return (
    typeof value === "string" &&
    (ASK_SCOPES as readonly string[]).includes(value)
  );
}

/**
 * Effective scope from the user's explicit choice and whether a meeting tab is
 * open. "meeting" needs an open meeting; without one it falls back to "library".
 */
export function resolveAskScope(
  preference: AskScope | null,
  meetingId: string | undefined,
): AskScope {
  if (preference === null) {
    return meetingId ? "meeting" : "library";
  }
  if (preference === "meeting" && !meetingId) {
    return "library";
  }
  return preference;
}

export type AskRedirectAction = "library" | "general";

export const ASK_REDIRECT_MEETING_UNRELATED =
  "That's not part of this meeting. Want me to check your other meetings, or answer it in General?";
export const ASK_REDIRECT_LIBRARY_UNRELATED =
  "That's not in your meetings. Want me to answer it in General?";
export const ASK_REDIRECT_OTHER_MEETING =
  "That's about a different meeting. Switch to All meetings to ask about it.";
export const ASK_USAGE_LIMIT_MESSAGE =
  "You've reached today's AI usage limit. Try again tomorrow, or add your own API key in Settings.";
export const ASK_NOT_DISCUSSED = "That wasn't discussed in this meeting.";
export const ASK_NOT_FOUND = "I couldn't find that in your meetings.";
export const ASK_GENERAL_BADGE = "General — not from your meetings.";
