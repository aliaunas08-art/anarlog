import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Tab } from "~/store/zustand/tabs";

const mocks = vi.hoisted(() => ({
  chatMode: "FloatingClosed" as
    | "FloatingClosed"
    | "FloatingOpen"
    | "RightPanelOpen",
  sendEvent: vi.fn(),
  sessionModes: {} as Record<string, string>,
  sessionEvents: {} as Record<string, any>,
  nowMs: 0,
  openUrl: vi.fn(),
  startCallbackServer: vi.fn(),
  getScheme: vi.fn(),
  startListening: vi.fn(),
  stopListening: vi.fn(),
  setPaused: vi.fn(),
  requestMainListenerControl: vi.fn(),
  isMainWebviewWindow: true,
  audioExists: false,
  hasTranscriptBySession: {} as Record<string, boolean>,
  meetingMicInUse: false,
  live: {
    sessionId: null as string | null,
    status: "inactive" as string,
    seconds: 0,
    amplitude: { mic: 0, speaker: 0 },
    muted: false,
    paused: false,
  },
  audioRetention: undefined as unknown,
}));

vi.mock("@tauri-apps/plugin-os", () => ({ platform: () => "macos" }));

vi.mock("~/shared/config", () => ({
  useConfigValue: () => mocks.audioRetention,
}));

vi.mock("~/contexts/shell", () => ({
  useShell: () => ({
    chat: { mode: mocks.chatMode, sendEvent: mocks.sendEvent },
  }),
}));

vi.mock("../shared", () => ({
  useHasTranscript: (sessionId: string) =>
    mocks.hasTranscriptBySession[sessionId] ?? false,
}));

vi.mock("@anlg/plugin-opener2", () => ({
  commands: { openUrl: mocks.openUrl },
}));

vi.mock("@anlg/plugin-deeplink2", () => ({
  commands: { startCallbackServer: mocks.startCallbackServer },
}));

vi.mock("~/calendar/hooks", () => ({
  useNow: () => new Date(mocks.nowMs),
}));

vi.mock("~/audio-player", () => ({
  useAudioPlayer: () => ({ audioExists: mocks.audioExists }),
}));

vi.mock("~/session/hooks/useSessionEvent", () => ({
  useSessionEvent: (sessionId: string) =>
    mocks.sessionEvents[sessionId] ?? null,
}));

vi.mock("~/session/hooks/useMeetingMicInUse", () => ({
  useMeetingMicInUse: () => mocks.meetingMicInUse,
}));

vi.mock("~/shared/utils", async (importOriginal) => ({
  ...(await importOriginal<typeof import("~/shared/utils")>()),
  getScheme: mocks.getScheme,
}));

vi.mock("~/stt/contexts", () => ({
  useListener: vi.fn((selector: (state: unknown) => unknown) =>
    selector({
      getSessionMode: (sessionId: string) =>
        mocks.sessionModes[sessionId] ?? "inactive",
      stop: mocks.stopListening,
      setPaused: mocks.setPaused,
      live: mocks.live,
    }),
  ),
}));

vi.mock("~/stt/useStartListening", () => ({
  useStartListening: () => mocks.startListening,
}));

vi.mock("~/stt/window-control", () => ({
  isMainWebviewWindow: () => mocks.isMainWebviewWindow,
  requestMainListenerControl: mocks.requestMainListenerControl,
}));

import { SessionBottomBar } from "./index";

const MEET_LINK = "https://meet.google.com/abc-defg-hij";
const DEMO_EVENT = {
  tracking_id: "anarlog-onboarding-demo-v1",
  meeting_link: "https://anarlog.so/onboarding-demo/",
};
const scheduled = (meeting_link?: string) => ({
  title: "Design Review",
  started_at: "2026-06-05T10:00:00.000Z",
  ended_at: "2026-06-05T10:30:00.000Z",
  meeting_link,
});
const BEFORE = "2026-06-05T09:55:00.000Z";
const DURING = "2026-06-05T10:05:00.000Z";
const AFTER = "2026-06-05T10:31:00.000Z";

const tab = {
  type: "sessions",
  id: "session-1",
  active: true,
  pinned: false,
  slotId: "slot-1",
  state: { view: null, autoStart: null },
} as Extract<Tab, { type: "sessions" }>;

type Scenario = {
  mode?: string;
  event?: Record<string, unknown>;
  now?: string;
  transcript?: boolean;
  audio?: boolean;
  micInUse?: boolean;
};

function arrange({
  mode,
  event,
  now = BEFORE,
  transcript,
  audio,
  micInUse,
}: Scenario) {
  if (mode) mocks.sessionModes = { "session-1": mode };
  if (event) mocks.sessionEvents = { "session-1": event };
  mocks.nowMs = new Date(now).getTime();
  mocks.hasTranscriptBySession = { "session-1": !!transcript };
  mocks.audioExists = !!audio;
  mocks.meetingMicInUse = !!micInUse;
}

function renderBar(
  viewSwitcher?: React.ReactNode,
  props: Partial<React.ComponentProps<typeof SessionBottomBar>> = {},
) {
  return render(
    <SessionBottomBar tab={tab} viewSwitcher={viewSwitcher} {...props} />,
  );
}

function recording(seconds = 754) {
  arrange({ mode: "active" });
  mocks.live = {
    sessionId: "session-1",
    status: "active",
    seconds,
    amplitude: { mic: 0.4, speaker: 0.2 },
    muted: false,
    paused: false,
  };
}

const ACTIONS = ["Record", "Join & record", "Stop"] as const;

function visibleAction() {
  const visible = ACTIONS.filter((name) =>
    screen.queryByRole("button", { name }),
  );
  expect(visible.length).toBeLessThanOrEqual(1);
  return visible[0] ?? null;
}

describe("SessionBottomBar", () => {
  beforeEach(() => {
    mocks.chatMode = "FloatingClosed";
    mocks.sendEvent.mockClear();
    mocks.sessionModes = {};
    mocks.sessionEvents = {};
    mocks.nowMs = new Date(BEFORE).getTime();
    mocks.openUrl.mockClear();
    mocks.startCallbackServer.mockReset();
    mocks.startCallbackServer.mockResolvedValue({ status: "ok", data: 43210 });
    mocks.getScheme.mockReset();
    mocks.getScheme.mockResolvedValue("anarlog-dev");
    mocks.startListening.mockClear();
    mocks.stopListening.mockClear();
    mocks.requestMainListenerControl.mockClear();
    mocks.isMainWebviewWindow = true;
    mocks.audioExists = false;
    mocks.hasTranscriptBySession = {};
    mocks.meetingMicInUse = false;
    mocks.setPaused.mockClear();
    mocks.live = {
      sessionId: null,
      status: "inactive",
      seconds: 0,
      amplitude: { mic: 0, speaker: 0 },
      muted: false,
      paused: false,
    };
    mocks.audioRetention = undefined;
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("renders the view tabs inside the meeting controls toolbar", () => {
    const onSelect = vi.fn();
    renderBar(
      <div role="group" aria-label="Session note views">
        <button type="button" onClick={onSelect}>
          Transcript
        </button>
      </div>,
    );

    const toolbar = screen.getByRole("toolbar", { name: "Meeting controls" });
    const views = screen.getByRole("group", { name: "Session note views" });
    expect(toolbar.contains(views)).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Transcript" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("shows the ⌘P shortcut on Ask", () => {
    renderBar();

    const ask = screen.getByRole("button", { name: "Ask Notes anything" });
    expect(ask.getAttribute("aria-keyshortcuts")).toBe("Meta+P");
    expect(ask.textContent).toContain("⌘P");
  });

  it("opens chat from Ask", () => {
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Ask Notes anything" }));

    expect(mocks.sendEvent).toHaveBeenCalledWith({ type: "OPEN" });
  });

  it("hides Ask while chat is docked in the right panel", () => {
    mocks.chatMode = "RightPanelOpen";
    renderBar();

    expect(
      screen.queryByRole("button", { name: "Ask Notes anything" }),
    ).toBeNull();
  });

  it("keeps tabs and the recording pill usable while floating chat is open", () => {
    recording();
    mocks.chatMode = "FloatingOpen";
    renderBar(
      <div role="group" aria-label="Session note views">
        <button type="button">Transcript</button>
      </div>,
    );

    const toolbar = screen.getByRole("toolbar", { name: "Meeting controls" });
    expect(toolbar.hasAttribute("inert")).toBe(false);
    expect(toolbar.getAttribute("aria-hidden")).toBeNull();
    expect(toolbar.className).not.toContain("invisible");
    expect(screen.getByRole("button", { name: "Transcript" })).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(mocks.stopListening).toHaveBeenCalledTimes(1);

    expect(
      screen.queryByRole("button", { name: "Ask Notes anything" }),
    ).toBeNull();
  });

  it("anchors the floating chat just above the bar", () => {
    renderBar();

    const marker = document.querySelector("[data-chat-cta-trigger]");
    expect(marker?.parentElement?.hasAttribute("data-session-bottom-bar")).toBe(
      true,
    );
    expect(marker?.className).toContain("bottom-full");
  });

  it("keeps a selection slot stacked above the bar", () => {
    renderBar();

    const slot = document.querySelector("[data-session-fab-selection]");
    const stack = slot?.parentElement;

    expect(stack?.className).toContain("flex-col-reverse");
    expect(slot?.className).toContain("mb-2");
  });

  it.each<[string, Scenario, (typeof ACTIONS)[number] | null]>([
    ["new ad hoc note", {}, "Record"],
    ["active recording", { mode: "active" }, "Stop"],
    [
      "active recording past the scheduled end",
      { mode: "active", event: scheduled(MEET_LINK), now: AFTER },
      "Stop",
    ],
    ["post-meeting transcription", { mode: "running_batch", now: AFTER }, null],
    ["finalizing", { mode: "finalizing", event: scheduled(MEET_LINK) }, null],
    ["inactive with transcript", { transcript: true }, "Record"],
    ["inactive with audio", { audio: true }, "Record"],
    ["meeting over", { event: scheduled(MEET_LINK), now: AFTER }, "Record"],
    ["meeting without a link", { event: scheduled() }, "Record"],
    [
      "meeting with an unrecognized link",
      { event: scheduled("https://naver.me/example") },
      "Record",
    ],
    [
      "remote meeting before start",
      { event: scheduled(MEET_LINK) },
      "Join & record",
    ],
    [
      "remote meeting before start with mic in use",
      { event: scheduled(MEET_LINK), micInUse: true },
      "Join & record",
    ],
    [
      "remote meeting after start with mic free",
      { event: scheduled(MEET_LINK), now: DURING },
      "Join & record",
    ],
    [
      "remote meeting after start with mic in use",
      { event: scheduled(MEET_LINK), now: DURING, micInUse: true },
      "Record",
    ],
    [
      "welcome demo after start with mic in use",
      {
        event: { ...DEMO_EVENT, started_at: "2026-06-05T10:00:00.000Z" },
        now: DURING,
        micInUse: true,
      },
      "Join & record",
    ],
  ])("shows the right primary action for %s", (_label, scenario, expected) => {
    arrange(scenario);
    renderBar();

    expect(visibleAction()).toBe(expected);
  });

  it("starts listening from Record without opening a meeting", () => {
    arrange({ event: scheduled("https://naver.me/example") });
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Record" }));

    expect(mocks.startListening).toHaveBeenCalledTimes(1);
    expect(mocks.startListening).toHaveBeenCalledWith();
    expect(mocks.openUrl).not.toHaveBeenCalled();
  });

  it("delegates Record to the main window from standalone windows", () => {
    mocks.isMainWebviewWindow = false;
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Record" }));

    expect(mocks.requestMainListenerControl).toHaveBeenCalledWith(
      "start",
      "session-1",
    );
    expect(mocks.startListening).not.toHaveBeenCalled();
  });

  it("starts a mic-only recording from In person", () => {
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Record in person" }));

    expect(mocks.startListening).toHaveBeenCalledWith({ micOnly: true });
  });

  it.each<[string, Scenario, boolean]>([
    ["ready to record", {}, true],
    ["recording", { mode: "active" }, false],
    ["join & record is offered", { event: scheduled(MEET_LINK) }, false],
  ])("shows In person only when %s", (_label, scenario, visible) => {
    arrange(scenario);
    renderBar();

    expect(
      screen.queryByRole("button", { name: "Record in person" }) !== null,
    ).toBe(visible);
  });

  it("hides In person outside the main window", () => {
    mocks.isMainWebviewWindow = false;
    renderBar();

    expect(screen.queryByRole("button", { name: "Record in person" })).toBe(
      null,
    );
  });

  it("opens the meeting link and starts listening from join & record", () => {
    arrange({ event: scheduled(MEET_LINK) });
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Join & record" }));

    expect(mocks.openUrl).toHaveBeenCalledWith(MEET_LINK, null);
    expect(mocks.startListening).toHaveBeenCalledTimes(1);
  });

  it("stops listening from the stop button", () => {
    arrange({ mode: "active" });
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    expect(mocks.stopListening).toHaveBeenCalledTimes(1);
  });

  it("delegates stop to the main window from standalone windows", () => {
    arrange({ mode: "active" });
    mocks.isMainWebviewWindow = false;
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    expect(mocks.requestMainListenerControl).toHaveBeenCalledWith(
      "stop",
      "session-1",
    );
    expect(mocks.stopListening).not.toHaveBeenCalled();
  });

  it("opens the welcome demo with an automatic completion callback", async () => {
    arrange({ event: DEMO_EVENT });
    renderBar();

    fireEvent.click(screen.getByRole("button", { name: "Join & record" }));

    expect(mocks.startListening).toHaveBeenCalledOnce();
    await vi.waitFor(() => {
      expect(mocks.startCallbackServer).toHaveBeenCalledWith(
        "anarlog-dev",
        null,
      );
      expect(mocks.openUrl).toHaveBeenCalledOnce();
    });

    const openedUrl = new URL(mocks.openUrl.mock.calls[0][0]);
    expect(openedUrl.origin + openedUrl.pathname).toBe(
      "https://anarlog.so/onboarding-demo/",
    );
    expect(openedUrl.searchParams.get("autojoin")).toBe("1");
    expect(openedUrl.searchParams.get("completion_url")).toBe(
      "http://127.0.0.1:43210/onboarding-demo/complete",
    );
  });

  it("still auto-joins the welcome demo if the completion callback cannot start", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    mocks.startCallbackServer.mockRejectedValue(new Error("unavailable"));
    arrange({ event: DEMO_EVENT });

    try {
      renderBar();
      fireEvent.click(screen.getByRole("button", { name: "Join & record" }));

      await vi.waitFor(() => {
        expect(mocks.openUrl).toHaveBeenCalledOnce();
      });

      const openedUrl = new URL(mocks.openUrl.mock.calls[0][0]);
      expect(openedUrl.searchParams.get("autojoin")).toBe("1");
      expect(openedUrl.searchParams.get("completion_url")).toBeNull();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("ignores repeated welcome demo joins while startup is in progress", async () => {
    let resolveCallbackServer: (value: {
      status: "ok";
      data: number;
    }) => void = () => {};
    mocks.startCallbackServer.mockReturnValue(
      new Promise((resolve) => {
        resolveCallbackServer = resolve;
      }),
    );
    arrange({ event: DEMO_EVENT });
    renderBar();

    const joinButton = screen.getByRole("button", { name: "Join & record" });
    fireEvent.click(joinButton);
    fireEvent.click(joinButton);

    expect(mocks.startListening).toHaveBeenCalledOnce();
    await vi.waitFor(() => {
      expect(mocks.startCallbackServer).toHaveBeenCalledOnce();
    });

    resolveCallbackServer({ status: "ok", data: 43210 });

    await vi.waitFor(() => {
      expect(mocks.openUrl).toHaveBeenCalledOnce();
      expect(joinButton.hasAttribute("disabled")).toBe(false);
    });
  });

  it.each([
    ["before recording", false, true],
    ["after recording", true, false],
  ])("prompts to try the welcome demo only %s", (_label, audio, prompted) => {
    arrange({ event: DEMO_EVENT, audio });
    renderBar();

    expect(screen.queryByText("Try the demo") !== null).toBe(prompted);
  });

  it("shows the meeting countdown before start and hides it while listening", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-05T09:55:30.000Z"));
    arrange({ event: scheduled(MEET_LINK), now: new Date().toISOString() });
    const { unmount } = renderBar();

    expect(screen.getByText("starts in 4m 30s")).not.toBeNull();
    unmount();

    arrange({ mode: "active", now: new Date().toISOString() });
    renderBar();

    expect(screen.queryByText(/starts in/)).toBeNull();
  });

  // Scheduled auto-start is owned by ScheduledMeetingAutoStart so it fires
  // regardless of which tab is open; the bar must not start a second one.
  it("does not auto-start when the countdown reaches the meeting start time", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-05T09:59:58.000Z"));
    arrange({ event: scheduled(MEET_LINK), now: new Date().toISOString() });
    renderBar();

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(mocks.startListening).not.toHaveBeenCalled();
    expect(mocks.openUrl).not.toHaveBeenCalled();
  });
  describe("recording pill", () => {
    it("replaces Record with a pill that shows the elapsed time", () => {
      recording(754);
      renderBar();

      const pill = screen.getByRole("group", { name: "Recording" });
      expect(screen.getByRole("timer").textContent).toBe("12:34");
      expect(pill.querySelector(".cf-record-dot")).not.toBeNull();
      expect(screen.queryByRole("button", { name: "Record" })).toBeNull();
    });

    it("ignores the timer of a different live session", () => {
      recording(754);
      mocks.live.sessionId = "other-session";
      renderBar();

      expect(screen.getByRole("timer").textContent).toBe("0:00");
    });

    it("stops listening from the pill", () => {
      recording();
      renderBar();

      fireEvent.click(screen.getByRole("button", { name: "Stop" }));

      expect(mocks.stopListening).toHaveBeenCalledTimes(1);
    });

    it("bookmarks the current moment from the button and from ⌘B", () => {
      recording(754);
      const onBookmark = vi.fn();
      renderBar(undefined, { onBookmark });

      fireEvent.click(
        screen.getByRole("button", { name: "Bookmark this moment" }),
      );
      fireEvent.keyDown(document.body, {
        key: "b",
        code: "KeyB",
        metaKey: true,
        ctrlKey: true,
      });

      expect(onBookmark).toHaveBeenCalledWith(754);
      expect(onBookmark).toHaveBeenCalledTimes(2);
    });

    it("pauses from the pill and from ⌘⇧P", () => {
      recording(754);
      renderBar();

      const pause = screen.getByRole("button", { name: "Pause recording" });
      expect(pause.querySelector('[data-icon="pause"]')).not.toBeNull();
      fireEvent.click(pause);
      fireEvent.keyDown(document.body, {
        key: "p",
        code: "KeyP",
        metaKey: true,
        shiftKey: true,
      });

      expect(mocks.setPaused).toHaveBeenNthCalledWith(1, true);
      expect(mocks.setPaused).toHaveBeenNthCalledWith(2, true);
    });

    it("shows a frozen, gray paused state with a Resume button", () => {
      recording(754);
      mocks.live.paused = true;
      renderBar();

      const pill = screen.getByRole("group", { name: "Recording paused" });
      expect(pill.querySelector(".cf-record-dot")).toBeNull();
      expect(pill.querySelector("[data-recording-level-meter]")).toBeNull();
      expect(screen.getByText("Paused")).toBeTruthy();
      expect(screen.getByRole("timer").textContent).toBe("12:34");
      expect(screen.getByRole("timer").getAttribute("aria-label")).toBe(
        "Recording paused at 12:34",
      );

      const resume = screen.getByRole("button", { name: "Resume recording" });
      expect(resume.querySelector('[data-icon="play"]')).not.toBeNull();
      fireEvent.click(resume);
      expect(mocks.setPaused).toHaveBeenCalledWith(false);
    });

    it("leaves Ctrl+Shift+P to the focused field on macOS", () => {
      recording();
      renderBar();

      fireEvent.keyDown(document.body, {
        key: "p",
        code: "KeyP",
        ctrlKey: true,
        shiftKey: true,
      });

      expect(mocks.setPaused).not.toHaveBeenCalled();
    });

    it("does not bind ⌘⇧P for a background session tab", () => {
      recording();
      renderBar(undefined, { tab: { ...tab, active: false } });

      fireEvent.keyDown(document.body, {
        key: "p",
        code: "KeyP",
        metaKey: true,
        shiftKey: true,
      });

      expect(mocks.setPaused).not.toHaveBeenCalled();
    });

    it("does not bind ⌘B for a background session tab", () => {
      recording();
      const onBookmark = vi.fn();
      renderBar(undefined, { onBookmark, tab: { ...tab, active: false } });

      fireEvent.keyDown(document.body, {
        key: "b",
        code: "KeyB",
        metaKey: true,
        ctrlKey: true,
      });

      expect(onBookmark).not.toHaveBeenCalled();
    });

    it.each([
      ["kept", "forever", true],
      ["deleted after the meeting", "none", false],
    ])(
      "marks audio as saved only when it is %s",
      (_label, retention, shown) => {
        recording();
        mocks.audioRetention = retention;
        renderBar();

        const mark = screen.queryByRole("img", {
          name: "Audio saved to this Mac",
        });
        expect(mark !== null).toBe(shown);
        if (mark) {
          // A quiet mark inside the pill, not a caption floating under the bar.
          expect(mark.closest("[data-session-recording-pill]")).not.toBeNull();
          expect(mark.getAttribute("title")).toBe("Audio saved to this Mac");
          expect(mark.className).toContain("text-[var(--cf-secondary)]");
        }
        expect(screen.queryByText("Audio saved to this Mac")).toBeNull();
      },
    );

    it("does not claim audio is saved before recording", () => {
      mocks.audioRetention = "forever";
      renderBar();

      expect(
        screen.queryByRole("img", { name: "Audio saved to this Mac" }),
      ).toBeNull();
    });
  });
});
