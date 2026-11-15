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
  updateSession: vi.fn(async () => {}),
  getAccountFullName: vi.fn(async () => null as string | null),
  selectedTemplateId: "" as string,
  templates: {} as Record<string, { title: string }>,
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
vi.mock("~/types/tauri.gen", () => ({
  commands: { getAccountFullName: mocks.getAccountFullName },
}));
vi.mock("~/shared/config", () => ({
  useConfigValue: () => mocks.selectedTemplateId,
}));
vi.mock("~/templates", () => ({
  useUserTemplate: (id: string | null) => ({
    data: id ? (mocks.templates[id] ?? null) : null,
  }),
}));
vi.mock("~/session/components/note-input/template-picker", () => ({
  TemplatePickerPopover: ({
    trigger,
    onSelectTemplate,
  }: {
    trigger: React.ReactNode;
    onSelectTemplate: (selection: {
      templateId: string | null;
      title: string;
    }) => void;
  }) => (
    <div>
      {trigger}
      <button
        type="button"
        onClick={() =>
          onSelectTemplate({
            templateId: "tpl-minutes",
            title: "Meeting Minutes",
          })
        }
      >
        pick-minutes
      </button>
    </div>
  ),
}));
vi.mock("~/session/queries", () => ({
  updateSession: mocks.updateSession,
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
    mocks.selectedTemplateId = "";
    mocks.templates = {};
    mocks.getAccountFullName.mockResolvedValue(null);
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

  it("falls back to the macOS account's first name", async () => {
    mocks.name = null;
    mocks.getAccountFullName.mockResolvedValue("Ali Aunas");
    render(<HomeView />);
    await waitFor(() =>
      expect(screen.getByRole("heading", { level: 1 }).textContent).toMatch(
        /, Ali$/,
      ),
    );
  });

  it("labels the template picker with the default template", () => {
    mocks.selectedTemplateId = "tpl-default";
    mocks.templates = { "tpl-default": { title: "Standup" } };
    render(<HomeView />);
    expect(
      screen.getByRole("button", { name: "Template: Standup" }),
    ).toBeTruthy();
  });

  it("creates the note with the picked template", async () => {
    render(<HomeView />);
    expect(screen.getByRole("button", { name: "Template: Auto" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "pick-minutes" }));
    expect(
      screen.getByRole("button", { name: "Template: Meeting Minutes" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /New note/ }));
    await waitFor(() =>
      expect(mocks.openCurrent).toHaveBeenCalledWith({
        type: "sessions",
        id: "new-session",
      }),
    );
    expect(mocks.updateSession).toHaveBeenCalledWith("new-session", {
      raw_md: "",
      raw_template_id: "tpl-minutes",
    });
  });

  it("asks chat with Return instead of making a note", () => {
    render(<HomeView />);
    const input = screen.getByLabelText("Question or note title");
    fireEvent.change(input, { target: { value: "what is an orange" } });
    fireEvent.submit(input.closest("form")!);
    expect(mocks.queueChatPrompt).toHaveBeenCalledWith(
      "ask:library",
      "what is an orange",
    );
    expect(mocks.sendEvent).toHaveBeenCalledWith({ type: "OPEN" });
    expect(mocks.createSession).not.toHaveBeenCalled();
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("does nothing on Return with an empty field", () => {
    render(<HomeView />);
    const input = screen.getByLabelText("Question or note title");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.submit(input.closest("form")!);
    expect(mocks.queueChatPrompt).not.toHaveBeenCalled();
    expect(mocks.sendEvent).not.toHaveBeenCalled();
    expect(mocks.createSession).not.toHaveBeenCalled();
  });

  it("creates a note titled with the typed text from the New note button", async () => {
    render(<HomeView />);
    fireEvent.change(screen.getByLabelText("Question or note title"), {
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
    expect(mocks.updateSession).not.toHaveBeenCalled();
    expect(mocks.queueChatPrompt).not.toHaveBeenCalled();
  });

  it("creates a note titled with the typed text on Cmd+N from the field", async () => {
    render(<HomeView />);
    const input = screen.getByLabelText("Question or note title");
    fireEvent.change(input, { target: { value: "Budget review" } });
    fireEvent.keyDown(input, { key: "n", metaKey: true });
    await waitFor(() =>
      expect(mocks.createSession).toHaveBeenCalledWith("Budget review"),
    );
    expect(mocks.queueChatPrompt).not.toHaveBeenCalled();
  });

  it("starts an in-person (mic only) recording", async () => {
    render(<HomeView />);
    fireEvent.click(screen.getByRole("button", { name: "In person" }));
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
      "ask:library",
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
