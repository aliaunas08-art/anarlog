import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { EditorView } from "~/store/zustand/tabs/schema";

const mocks = vi.hoisted(() => ({
  leftsidebar: { expanded: true },
  sessionModes: {} as Record<string, string>,
  sessionEvents: {} as Record<string, any>,
  nowMs: 0,
  audioExists: false,
  hasTranscriptBySession: {} as Record<string, boolean>,
}));

vi.mock("~/session/queries", () => ({
  useSession: () => ({ folder_id: "" }),
  useFolderIcons: () => ({}),
  useFolderPaths: () => [],
  useUpdateSession: () => vi.fn(),
}));

vi.mock("./overflow", () => ({
  OverflowButton: () => <button type="button">More</button>,
}));

vi.mock("~/session-sharing", () => ({
  SessionShareButton: () => (
    <button type="button" aria-label="Share note">
      Share
    </button>
  ),
}));

vi.mock("../shared", () => ({
  useHasTranscript: (sessionId: string) =>
    mocks.hasTranscriptBySession[sessionId] ?? false,
}));

vi.mock("~/calendar/hooks", () => ({
  useNow: () => new Date(mocks.nowMs),
}));

vi.mock("~/audio-player", () => ({
  useAudioPlayer: () => ({ audioExists: mocks.audioExists }),
}));

vi.mock("~/contexts/shell", () => ({
  useShell: () => ({ leftsidebar: mocks.leftsidebar }),
}));

vi.mock("~/session/hooks/useSessionEvent", () => ({
  useSessionEvent: (sessionId: string) =>
    mocks.sessionEvents[sessionId] ?? null,
}));

vi.mock("~/stt/contexts", () => ({
  useListener: vi.fn((selector: (state: unknown) => unknown) =>
    selector({
      getSessionMode: (sessionId: string) =>
        mocks.sessionModes[sessionId] ?? "inactive",
    }),
  ),
}));

import { OuterHeader } from "./index";

const MEET_LINK = "https://meet.google.com/abc-defg-hij";
const scheduled = (meeting_link?: string) => ({
  title: "Design Review",
  started_at: "2026-06-05T10:00:00.000Z",
  ended_at: "2026-06-05T10:30:00.000Z",
  meeting_link,
});
const BEFORE = "2026-06-05T09:55:00.000Z";
const AFTER = "2026-06-05T10:31:00.000Z";

type Scenario = {
  mode?: string;
  event?: Record<string, unknown>;
  now?: string;
  transcript?: boolean;
  audio?: boolean;
};

function arrange({ mode, event, now = BEFORE, transcript, audio }: Scenario) {
  if (mode) mocks.sessionModes = { "session-1": mode };
  if (event) mocks.sessionEvents = { "session-1": event };
  mocks.nowMs = new Date(now).getTime();
  mocks.hasTranscriptBySession = { "session-1": !!transcript };
  mocks.audioExists = !!audio;
}

function renderHeader(
  props: { view?: EditorView; standaloneWindow?: boolean } = {},
) {
  const view = props.view ?? ({ type: "raw" } as EditorView);
  return render(
    <OuterHeader
      sessionId="session-1"
      currentView={view}
      standaloneWindow={props.standaloneWindow}
    />,
  );
}

describe("OuterHeader", () => {
  beforeEach(() => {
    mocks.leftsidebar.expanded = true;
    mocks.sessionModes = {};
    mocks.sessionEvents = {};
    mocks.nowMs = new Date(BEFORE).getTime();
    mocks.audioExists = false;
    mocks.hasTranscriptBySession = {};
  });

  afterEach(() => {
    cleanup();
  });

  it.each<[string, Scenario, boolean]>([
    ["new ad hoc note", {}, false],
    ["active recording", { mode: "active" }, false],
    [
      "post-meeting transcription",
      { mode: "running_batch", now: AFTER },
      false,
    ],
    ["finalizing", { mode: "finalizing", event: scheduled(MEET_LINK) }, false],
    ["inactive with transcript", { transcript: true }, true],
    ["inactive with audio", { audio: true }, true],
    ["meeting over", { event: scheduled(MEET_LINK), now: AFTER }, true],
    [
      "recorded event without ended_at",
      {
        event: { ...scheduled(MEET_LINK), ended_at: undefined },
        now: AFTER,
        transcript: true,
      },
      true,
    ],
    ["remote meeting before start", { event: scheduled(MEET_LINK) }, false],
  ])(
    "shows share only once the meeting is over: %s",
    (_label, scenario, shared) => {
      arrange(scenario);
      renderHeader();

      expect(
        screen.queryByRole("button", { name: "Share note" }) !== null,
      ).toBe(shared);
      expect(screen.getByRole("button", { name: "More" })).not.toBeNull();
    },
  );

  // Recording controls and view tabs moved to the bottom bar; the title moved
  // to the editorial header in the note body.
  it("does not render recording controls, view tabs or the title", () => {
    renderHeader();

    for (const name of [
      "Record",
      "Join & record",
      "Stop",
      "Record in person",
    ]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    expect(screen.queryByRole("textbox", { name: "Session title" })).toBeNull();
    expect(screen.queryByRole("group", { name: "Session note views" })).toBe(
      null,
    );
  });
});
