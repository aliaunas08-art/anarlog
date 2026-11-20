import { liveQueryClient } from "~/db";

// ASPC C6.4: all-meetings Ask leaves Private meetings out. Rust already
// drops them from `tantivy:search` and `db:list_meetings`; this wrapper is
// the guardrail for every library tool: a Private meeting id in the input is
// refused, and Private meetings are removed from the output.

export const PRIVATE_MEETING_TOOL_MESSAGE =
  "That meeting is marked Private and is not available in All meetings. The user can open it and ask in This meeting.";

const ID_FIELDS = [
  "meeting_id",
  "meetingId",
  "sessionId",
  "session_id",
  "sourceMeetingId",
  "targetMeetingId",
] as const;

export async function loadPrivateMeetingIds(): Promise<Set<string>> {
  const rows = await liveQueryClient.execute<{ id: string }>(
    "SELECT id FROM sessions WHERE private <> 0",
    [],
  );
  return new Set(rows.map((row) => row.id));
}

type ExecutableTool = {
  execute?: (input: unknown, options?: unknown) => unknown;
  [key: string]: unknown;
};

function referencesPrivate(input: unknown, ids: Set<string>): boolean {
  if (typeof input !== "object" || input === null) return false;
  const params = input as Record<string, unknown>;
  for (const field of ID_FIELDS) {
    const value = params[field];
    if (typeof value === "string" && ids.has(value)) return true;
  }
  const list = params.meeting_ids ?? params.meetingIds;
  return Array.isArray(list) && list.some((id) => ids.has(id as string));
}

function belongsToPrivate(value: unknown, ids: Set<string>): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const record = value as Record<string, unknown>;
  return ["id", ...ID_FIELDS].some(
    (field) =>
      typeof record[field] === "string" && ids.has(record[field] as string),
  );
}

/** Removes list entries (at any depth) that belong to a Private meeting. */
export function stripPrivateMeetings<T>(value: T, ids: Set<string>): T {
  if (ids.size === 0) return value;
  if (Array.isArray(value)) {
    return value
      .filter((item) => !belongsToPrivate(item, ids))
      .map((item) => stripPrivateMeetings(item, ids)) as T;
  }
  if (typeof value === "object" && value !== null) {
    const out: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value)) {
      out[key] = stripPrivateMeetings(item, ids);
    }
    return out as T;
  }
  return value;
}

export function excludePrivateMeetings<T>(
  tool: T,
  loadIds: () => Promise<Set<string>> = loadPrivateMeetingIds,
): T {
  const original = tool as unknown as ExecutableTool;
  const execute = original.execute;
  if (typeof execute !== "function") return tool;

  return {
    ...original,
    execute: async (input: unknown, options?: unknown) => {
      const ids = await loadIds();
      if (referencesPrivate(input, ids)) {
        return { status: "error", message: PRIVATE_MEETING_TOOL_MESSAGE };
      }
      const result = await execute(input, options);
      return stripPrivateMeetings(result, ids);
    },
  } as unknown as T;
}
