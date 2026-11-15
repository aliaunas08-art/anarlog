import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(async () => "new-session"),
  getOrCreateSessionForEventId: vi.fn(async () => "event-session"),
  openSessionAndListen: vi.fn(),
  markSessionInPerson: vi.fn(),
  queueChatPrompt: vi.fn(),
  sendEvent: vi.fn(),
  openCurrent: vi.fn(),
  openUrl: vi.fn(async () => ({ status: "ok", data: null })),
  unexpectedQuit: false as boolean | null,
  name: "Ali Aunas" as string | null,
  upcoming: null as null | Record<string, string>,
  recents: [] as Array<Record<string, string>>,
  marker: null as null | Record<string, unknown>,
  ignored: new Set<string>(),
}));

vi.mock("@tauri-apps/plugin-os", () => ({ platform: () => "macos" }));
vi.mock("@anlg/plugin-opener2", () => ({
  commands: { openUrl: mocks.openUrl },
}));
vi.mock("~/calendar/ignored-events", () => ({
  useIgnoredEvents: () => ({
    isIgnored: (trackingId: string) => mocks.ignored.has(trackingId),
  }),
}));
vi.mock("~/auth", () => ({ useAuth: () => ({ session: null }) }));
vi.mock("~/shared/owner-user", () => ({ useOwnerUserId: () => "owner" }));
vi.mock("~/contacts/queries", () => ({
  usePersonalContact: () => ({
    data: mocks.name === null ? null : { name: mocks.name },
  }),
}));
vi.mock("~/contexts/shell", () => ({
  useShell: () => ({
    chat: { sessionId: "chat-1", sendEvent: mocks.sendEvent },
  }),
}));
vi.mock("~/chat/components/input/hooks", () => ({
  queueChatPrompt: mocks.queueChatPrompt,
}));
vi.mock("~/db", () => ({
  useLiveQuery: ({
    sql,
    mapRows,
  }: {
    sql: string;
    mapRows: (rows: unknown[]) => unknown;
  }) => ({
    data: mapRows(
      sql.includes("FROM events")
        ? mocks.upcoming
          ? [mocks.upcoming]
          : []
        : mocks.recents,
    ),
  }),
}));
vi.mock("~/session/queries", () => ({
  createSession: mocks.createSession,
  getOrCreateSessionForEventId: mocks.getOrCreateSessionForEventId,
}));
vi.mock("~/shared/useNewNote", () => ({
  openSessionAndListen: mocks.openSessionAndListen,
}));
vi.mock("~/stt/useStartListening", () => ({
  markSessionInPerson: mocks.markSessionInPerson,
}));
vi.mock("~/stt/capture-lifecycle-storage", () => ({
  loadCaptureLifecycleMarker: async () => mocks.marker,
  hasAudioAwaitingUser: (marker: { chunkedAudio?: boolean }) =>
    marker.chunkedAudio === true,
}));
vi.mock("~/shared/unexpected-quit", () => ({
  useUnexpectedQuit: () => mocks.unexpectedQuit,
  dismissUnexpectedQuit: vi.fn(),
}));
vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (
    selector: (state: { openCurrent: typeof mocks.openCurrent }) => unknown,
  ) => selector({ openCurrent: mocks.openCurrent }),
}));

import { HomeView } from "./home";

describe("HomeView", () => {
  beforeEach(() => {
    mocks.unexpectedQuit = false;
    mocks.name = "Ali Aunas";
    mocks.upcoming = null;
    mocks.recents = [];
    mocks.marker = null;
    mocks.ignored = new Set();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("greets by first name, and says Welcome back after an unexpected quit", () => {
    mocks.unexpectedQuit = true;
    render(<HomeView />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Welcome back, Ali",
    );
    expect(
      screen.getByText(
        "Everything from last time is safe. Ready when you are.",
      ),
    ).toBeTruthy();
  });

  it("drops the name when the profile has none", () => {
    mocks.name = null;
    render(<HomeView />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).not.toContain(
      ",",
    );
  });

  it("creates a note titled with the typed text on Return", async () => {
    render(<HomeView />);
    fireEvent.change(screen.getByLabelText("Note title or question"), {
      target: { value: "Budget review" },
    });
    fireEvent.click(screen.getByRole("button", { name: /New note/ }));
    await waitFor(() =>
      expect(mocks.openCurrent).toHaveBeenCalledWith({
        type: "sessions",
        id: "new-session",
      }),
    );
    expect(mocks.createSession).toHaveBeenCalledWith("Budget review");
  });

  it("starts an in-person (mic only) recording", async () => {
    render(<HomeView />);
    fireEvent.click(screen.getByRole("button", { name: /In person/ }));
    await waitFor(() =>
      expect(mocks.openSessionAndListen).toHaveBeenCalledWith("new-session", {
        behavior: "current",
      }),
    );
    expect(mocks.markSessionInPerson).toHaveBeenCalledWith("new-session");
  });

  it("sends a suggestion to chat", () => {
    render(<HomeView />);
    fireEvent.click(screen.getByRole("button", { name: "Recap yesterday" }));
    expect(mocks.queueChatPrompt).toHaveBeenCalledWith(
      "chat-1",
      "Recap yesterday",
    );
    expect(mocks.sendEvent).toHaveBeenCalledWith({ type: "OPEN" });
  });

  it("records the upcoming meeting and opens its link", async () => {
    mocks.upcoming = {
      id: "evt-1",
      tracking_id_event: "trk-1",
      recurrence_series_id: "",
      title: "Product sync",
      started_at: new Date(Date.now() + 10 * 60_000).toISOString(),
      meeting_link: "https://meet.google.com/abc",
    };
    render(<HomeView />);
    fireEvent.click(screen.getByRole("button", { name: "Join & record" }));
    expect(mocks.openUrl).toHaveBeenCalledWith(
      "https://meet.google.com/abc",
      null,
    );
    await waitFor(() =>
      expect(mocks.openSessionAndListen).toHaveBeenCalledWith("event-session", {
        behavior: "current",
      }),
    );
    expect(
      screen.getByRole("button", { name: "Prep me for Product sync" }),
    ).toBeTruthy();
  });

  it("shows the recovered note after an unexpected quit", async () => {
    mocks.unexpectedQuit = true;
    mocks.recents = [
      {
        id: "s1",
        title: "Design review",
        updated_at: new Date().toISOString(),
      },
    ];
    mocks.marker = { chunkedAudio: true };
    render(<HomeView />);
    expect(await screen.findByText("Design review was recovered")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(mocks.openCurrent).toHaveBeenCalledWith({
      type: "sessions",
      id: "s1",
    });
  });

  it("lists recent notes to jump back in", () => {
    mocks.recents = [
      {
        id: "s1",
        title: "March Launch Planning",
        updated_at: new Date().toISOString(),
      },
    ];
    render(<HomeView />);
    fireEvent.click(
      screen.getByRole("button", { name: /March Launch Planning/ }),
    );
    expect(mocks.openCurrent).toHaveBeenCalledWith({
      type: "sessions",
      id: "s1",
    });
  });

  it("hides ignored meetings and keeps ones that just started", () => {
    mocks.upcoming = {
      id: "evt-2",
      tracking_id_event: "trk-2",
      recurrence_series_id: "",
      title: "Standup",
      started_at: new Date(Date.now() - 5 * 60_000).toISOString(),
      meeting_link: "",
    };
    render(<HomeView />);
    expect(screen.getByText(/^Standup at /)).toBeTruthy();
    expect(screen.getByText(/Started \d+ min ago/)).toBeTruthy();
    cleanup();
    mocks.ignored = new Set(["trk-2"]);
    render(<HomeView />);
    expect(screen.queryByText(/^Standup at /)).toBeNull();
  });
});
