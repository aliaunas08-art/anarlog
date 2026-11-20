import type { ToolSet } from "ai";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ execute: vi.fn() }));

vi.mock("~/db", () => ({
  liveQueryClient: { execute: mocks.execute },
}));

import {
  excludePrivateMeetings,
  PRIVATE_MEETING_TOOL_MESSAGE,
  stripPrivateMeetings,
} from "./private";
import { selectToolsForScope } from "./tools";

const PRIVATE = new Set(["secret"]);

function fakeTools(): ToolSet {
  const listMeetings = vi.fn(async () => ({
    meetings: [
      { id: "open", title: "Open" },
      { id: "secret", title: "Secret" },
    ],
    pagination: { next_offset: null },
  }));
  const getMeeting = vi.fn(async (input: { meeting_id: string }) => ({
    id: input.meeting_id,
    title: "Meeting",
  }));
  const searchMeetingContent = vi.fn(async () => ({
    hits: [
      { meetingId: "secret", excerpt: "SECRET-MARKER" },
      { meetingId: "open", excerpt: "fine" },
    ],
  }));
  return {
    list_meetings: { execute: listMeetings },
    get_meeting: { execute: getMeeting },
    get_meeting_transcript: { execute: getMeeting },
    search_meeting_content: { execute: searchMeetingContent },
  } as unknown as ToolSet;
}

async function run(tools: ToolSet, name: string, input: unknown) {
  const tool = tools[name] as unknown as {
    execute: (input: unknown, options?: unknown) => Promise<unknown>;
  };
  return tool.execute(input, {});
}

describe("Private meetings in all-meetings Ask (C6.4)", () => {
  beforeEach(() => {
    mocks.execute.mockReset();
    mocks.execute.mockResolvedValue([{ id: "secret" }]);
  });

  it("drops Private meetings from library listings and search results", async () => {
    const tools = selectToolsForScope(fakeTools(), {
      scope: "library",
      explicitEdit: false,
    });
    const listed = (await run(tools, "list_meetings", {})) as {
      meetings: Array<{ id: string }>;
    };
    expect(listed.meetings.map((m) => m.id)).toEqual(["open"]);
    const searched = await run(tools, "search_meeting_content", {
      query: "marker",
    });
    expect(JSON.stringify(searched)).not.toContain("SECRET-MARKER");
  });

  it("refuses to read a Private meeting by id in All meetings", async () => {
    const tools = selectToolsForScope(fakeTools(), {
      scope: "library",
      explicitEdit: false,
    });
    for (const name of ["get_meeting", "get_meeting_transcript"]) {
      expect(await run(tools, name, { meeting_id: "secret" })).toEqual({
        status: "error",
        message: PRIVATE_MEETING_TOOL_MESSAGE,
      });
    }
    expect(
      await run(tools, "search_meeting_content", { meeting_ids: ["secret"] }),
    ).toEqual({ status: "error", message: PRIVATE_MEETING_TOOL_MESSAGE });
  });

  it("keeps This meeting Ask working on the Private meeting itself", async () => {
    const tools = selectToolsForScope(fakeTools(), {
      scope: "meeting",
      meetingId: "secret",
      explicitEdit: false,
    });
    expect(await run(tools, "get_meeting", {})).toEqual({
      id: "secret",
      title: "Meeting",
    });
    expect(mocks.execute).not.toHaveBeenCalled();
  });

  it("includes Private meetings only when this request includes them", async () => {
    const tools = selectToolsForScope(fakeTools(), {
      scope: "library",
      explicitEdit: false,
      includePrivate: true,
    });
    const listed = (await run(tools, "list_meetings", {})) as {
      meetings: Array<{ id: string }>;
    };
    expect(listed.meetings).toHaveLength(2);
  });

  it("strips nested entries and leaves other values alone", () => {
    expect(
      stripPrivateMeetings(
        {
          results: [{ session_id: "secret" }, { session_id: "x", n: [1] }],
          total: 2,
        },
        PRIVATE,
      ),
    ).toEqual({ results: [{ session_id: "x", n: [1] }], total: 2 });
    const tool = excludePrivateMeetings(
      { execute: async () => "plain" },
      async () => PRIVATE,
    );
    return expect(
      (tool as { execute: (i: unknown) => Promise<unknown> }).execute({}),
    ).resolves.toBe("plain");
  });
});
