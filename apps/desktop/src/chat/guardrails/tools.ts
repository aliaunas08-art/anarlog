import type { ToolSet } from "ai";

import type { AskScope } from "./scope";

// Read tools that can be pinned to one meeting id.
const MEETING_READ_TOOLS = [
  "get_meeting",
  "get_meeting_transcript",
  "search_meeting_content",
] as const;

// Write tools that only touch the current meeting.
const MEETING_EDIT_TOOLS = [
  "edit_summary",
  "edit_memo",
  "apply_session_correction",
] as const;

const LIBRARY_READ_TOOLS = [
  "list_meetings",
  "get_meeting",
  "get_meeting_transcript",
  "get_recurring_meeting_history",
  "search_meeting_content",
  "find_related_meetings",
  "search_meetings",
  "list_folders",
  "read_folder_material",
  "search_contacts",
  "search_calendar_events",
] as const;

const LIBRARY_WRITE_TOOLS = [
  "create_folder",
  "move_meetings_to_folder",
  "edit_summary",
  "edit_memo",
  "apply_session_correction",
  "move_meeting_contents",
] as const;

// Id fields a tool may carry that must equal the pinned meeting.
const MEETING_ID_FIELDS = [
  "meeting_id",
  "sessionId",
  "sourceMeetingId",
  "targetMeetingId",
] as const;

export const OUT_OF_SCOPE_TOOL_MESSAGE =
  "This chat is scoped to the current meeting. Ask the user to switch to All meetings to look at other meetings.";

type ExecutableTool = {
  execute?: (input: unknown, options?: unknown) => unknown;
  [key: string]: unknown;
};

/**
 * Wraps a tool so it can only read or write `meetingId`: any other id in its
 * input is rejected in code, and `defaultFields` are filled with the pinned id.
 */
export function pinToolToMeeting<T>(
  tool: T,
  meetingId: string,
  {
    defaultFields = [],
    forceMeetingIds = false,
  }: { defaultFields?: string[]; forceMeetingIds?: boolean } = {},
): T {
  const original = tool as unknown as ExecutableTool;
  const execute = original.execute;
  if (typeof execute !== "function") return tool;

  return {
    ...original,
    execute: async (input: unknown, options?: unknown) => {
      const params =
        typeof input === "object" && input !== null
          ? { ...(input as Record<string, unknown>) }
          : {};
      for (const field of MEETING_ID_FIELDS) {
        const value = params[field];
        if (value !== undefined && value !== null && value !== meetingId) {
          return { status: "error", message: OUT_OF_SCOPE_TOOL_MESSAGE };
        }
      }
      if (
        Array.isArray(params.meeting_ids) &&
        params.meeting_ids.some((id) => id !== meetingId)
      ) {
        return { status: "error", message: OUT_OF_SCOPE_TOOL_MESSAGE };
      }
      if (forceMeetingIds) params.meeting_ids = [meetingId];
      for (const field of defaultFields) {
        if (params[field] === undefined) params[field] = meetingId;
      }
      return execute(params, options);
    },
  } as unknown as T;
}

const PIN_OPTIONS: Record<
  string,
  { defaultFields?: string[]; forceMeetingIds?: boolean }
> = {
  get_meeting: { defaultFields: ["meeting_id"] },
  get_meeting_transcript: { defaultFields: ["meeting_id"] },
  search_meeting_content: { forceMeetingIds: true },
  edit_summary: { defaultFields: ["sessionId"] },
  edit_memo: { defaultFields: ["sessionId"] },
  apply_session_correction: { defaultFields: ["sessionId"] },
};

function pick(tools: ToolSet, names: readonly string[]): ToolSet {
  const picked: ToolSet = {};
  for (const name of names) {
    const tool = tools[name];
    if (tool) picked[name] = tool;
  }
  return picked;
}

/**
 * Tool exposure per scope, enforced in code:
 * - meeting: read tools pinned to the current meeting, plus edit tools for that
 *   meeting only when the user explicitly asked to edit.
 * - library: read/search tools; write tools only when explicitly asked.
 */
export function selectToolsForScope(
  tools: ToolSet,
  {
    scope,
    meetingId,
    explicitEdit,
  }: { scope: AskScope; meetingId?: string | null; explicitEdit: boolean },
): ToolSet {
  if (scope === "meeting") {
    if (!meetingId) return {};
    const names = explicitEdit
      ? [...MEETING_READ_TOOLS, ...MEETING_EDIT_TOOLS]
      : [...MEETING_READ_TOOLS];
    const selected = pick(tools, names);
    const pinned: ToolSet = {};
    for (const [name, tool] of Object.entries(selected)) {
      pinned[name] = pinToolToMeeting(tool, meetingId, PIN_OPTIONS[name]);
    }
    return pinned;
  }

  return pick(
    tools,
    explicitEdit
      ? [...LIBRARY_READ_TOOLS, ...LIBRARY_WRITE_TOOLS]
      : [...LIBRARY_READ_TOOLS],
  );
}
