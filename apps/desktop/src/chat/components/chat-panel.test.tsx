import { act, cleanup, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  chatSession: vi.fn(),
  hasAvailableTranscript: false,
  sessionMode: "inactive",
  requestedLiveTranscription: null as boolean | null,
  liveTranscriptionActive: null as boolean | null,
  toolbarControls: vi.fn((_props: Record<string, unknown>) => (
    <div data-testid="chat-toolbar" />
  )),
  chat: {
    groupId: "group-1",
    scope: "general" as "general" | "automations",
    sessionId: "session-1",
    startNewChat: vi.fn(),
    selectChat: vi.fn(),
  },
  noteFilter: "mine",
  folderFilter: null as string | null,
  currentSessionId: "session-1" as string | undefined,
  statusBySession: {} as Record<string, string>,
}));

vi.mock("./toolbar-controls", () => ({
  ChatToolbarControls: (props: Record<string, unknown>) => {
    mocks.toolbarControls(props);
    return (
      <div data-surface={props.surface as string} data-testid="chat-toolbar" />
    );
  },
}));

vi.mock("./body", () => ({
  ChatBody: () => <div data-testid="chat-body" />,
}));

vi.mock("./content", () => ({
  ChatContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

vi.mock("./session-provider", () => ({
  ChatSession: (props: { children: (props: object) => React.ReactNode }) => {
    mocks.chatSession(props);
    const sessionId = (props as { sessionId?: string }).sessionId ?? "";
    return props.children({
      sessionId,
      messages: [],
      status: mocks.statusBySession[sessionId] ?? "ready",
      error: undefined,
      regenerate: vi.fn(),
      contextEntities: [],
      sendMessage: vi.fn(),
      pendingRefs: [],
    });
  },
}));

vi.mock("~/ai/hooks", () => ({
  useLanguageModel: () => ({ id: "model-1" }),
}));

vi.mock("~/chat/store/use-chat-actions", () => ({
  useChatActions: () => ({
    handleSendMessage: vi.fn(),
  }),
}));

vi.mock("./use-session-tab", () => ({
  useSessionTab: () => ({ currentSessionId: mocks.currentSessionId }),
}));

vi.mock("./use-summary-quick-actions", () => ({
  useHasEditableSummary: () => false,
}));

vi.mock("~/sidebar/note-filter", () => ({
  folderIdForNewNote: (noteFilter: string, folderFilter: string | null) =>
    noteFilter === "mine" && folderFilter !== null ? folderFilter : undefined,
  useSidebarNotes: (
    selector: (state: {
      noteFilter: string;
      folderFilter: string | null;
    }) => unknown,
  ) =>
    selector({
      noteFilter: mocks.noteFilter,
      folderFilter: mocks.folderFilter,
    }),
}));

vi.mock("~/contexts/shell", () => ({
  useShell: () => ({ chat: mocks.chat }),
}));

vi.mock("~/shared/owner-user", () => ({
  useOwnerUserId: () => "user-1",
}));

vi.mock("~/session/queries", () => ({
  useSessionHasTranscript: (sessionId: string) =>
    Boolean(sessionId) && mocks.hasAvailableTranscript,
}));

vi.mock("~/stt/contexts", () => ({
  useListener: (selector: (state: unknown) => unknown) =>
    selector({
      getSessionMode: () => mocks.sessionMode,
      live: {
        requestedLiveTranscription: mocks.requestedLiveTranscription,
        liveTranscriptionActive: mocks.liveTranscriptionActive,
        batchTranscriptionPendingBySession: {},
      },
    }),
}));

import { ChatPanelFrame, ChatSessionHost } from "./chat-panel";

import { useChatContext } from "~/chat/state/chat-context";

function TestChatPanel({
  layout = "floating",
}: {
  layout?: "floating" | "right-panel";
}) {
  return (
    <ChatSessionHost>
      {(sessionProps) => (
        <ChatPanelFrame layout={layout} sessionProps={sessionProps} />
      )}
    </ChatSessionHost>
  );
}

describe("Chat panel", () => {
  beforeEach(() => {
    cleanup();
    mocks.chatSession.mockClear();
    mocks.chat.scope = "general";
    mocks.noteFilter = "mine";
    mocks.folderFilter = null;
    mocks.hasAvailableTranscript = false;
    mocks.sessionMode = "inactive";
    mocks.requestedLiveTranscription = null;
    mocks.liveTranscriptionActive = null;
    mocks.toolbarControls.mockClear();
    mocks.currentSessionId = "session-1";
    mocks.statusBySession = {};
    useChatContext.setState({
      askThreads: {},
      askScopePreference: null,
      askScopePreferenceMeetingId: null,
    });
  });

  it("passes batch-only recording state to the chat session", () => {
    mocks.sessionMode = "active";
    mocks.requestedLiveTranscription = false;
    mocks.liveTranscriptionActive = false;

    render(<TestChatPanel />);

    expect(mocks.chatSession).toHaveBeenCalledWith(
      expect.objectContaining({
        hasAvailableTranscript: false,
        isBatchTranscriptionPending: true,
      }),
    );
  });

  it("preserves an existing transcript during batch retranscription", () => {
    mocks.hasAvailableTranscript = true;
    mocks.sessionMode = "active";
    mocks.requestedLiveTranscription = false;
    mocks.liveTranscriptionActive = false;

    render(<TestChatPanel />);

    expect(mocks.chatSession).toHaveBeenCalledWith(
      expect.objectContaining({
        hasAvailableTranscript: true,
        isBatchTranscriptionPending: true,
      }),
    );
  });

  it("passes the active folder into the chat session", () => {
    mocks.folderFilter = "CS 101";

    render(<TestChatPanel />);

    expect(mocks.chatSession).toHaveBeenCalledWith(
      expect.objectContaining({
        folderId: "CS 101",
      }),
    );
  });

  it("does not inherit note context in the automations scope", () => {
    mocks.chat.scope = "automations";
    mocks.folderFilter = "CS 101";
    mocks.hasAvailableTranscript = true;
    mocks.sessionMode = "active";

    render(<TestChatPanel layout="right-panel" />);

    expect(mocks.chatSession).toHaveBeenCalledWith(
      expect.objectContaining({
        currentSessionId: undefined,
        folderId: undefined,
        hasAvailableTranscript: false,
        isBatchTranscriptionPending: false,
      }),
    );
    expect(screen.queryByTestId("chat-toolbar")).toBeNull();
    expect(mocks.toolbarControls).not.toHaveBeenCalled();
  });

  it("#9 keeps a still-answering thread mounted across a meeting switch until it is idle", () => {
    const first = "ask:meeting:session-1";
    const second = "ask:meeting:session-2";
    mocks.statusBySession[first] = "streaming";
    const view = render(<TestChatPanel />);
    expect(mocks.chatSession).toHaveBeenLastCalledWith(
      expect.objectContaining({
        sessionId: first,
        currentSessionId: "session-1",
      }),
    );

    mocks.chatSession.mockClear();
    mocks.currentSessionId = "session-2";
    view.rerender(<TestChatPanel />);
    const mounted = () =>
      new Set(
        mocks.chatSession.mock.calls.map(
          ([props]) => (props as { sessionId: string }).sessionId,
        ),
      );
    // The streaming thread stays mounted (so onFinish persists its reply);
    // the new meeting gets its own thread.
    expect(mounted()).toEqual(new Set([first, second]));
    expect(mocks.chatSession).toHaveBeenCalledWith(
      expect.objectContaining({
        sessionId: second,
        currentSessionId: "session-2",
        askThreadKey: "meeting:session-2",
      }),
    );

    // Once the old stream is done, its session unmounts.
    mocks.statusBySession[first] = "ready";
    mocks.chatSession.mockClear();
    act(() => {
      useChatContext.setState({});
    });
    view.rerender(<TestChatPanel />);
    mocks.chatSession.mockClear();
    view.rerender(<TestChatPanel />);
    expect(mounted()).toEqual(new Set([second]));
  });
});
