import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  transitionChatMode: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-os", () => ({ platform: () => "macos" }));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (selector?: (state: unknown) => unknown) => {
    const state = {
      chatMode: "FloatingClosed",
      transitionChatMode: mocks.transitionChatMode,
      currentTab: null,
    };
    return selector ? selector(state) : state;
  },
}));

vi.mock("./chat-context", () => ({
  effectiveAskPreference: () => null,
  selectChatSelection: (
    state: { chatByScope: Record<string, unknown> },
    scope: string,
  ) => state.chatByScope[scope],
  useChatContext: (selector: (state: unknown) => unknown) =>
    selector({
      chatByScope: { general: { groupId: undefined, sessionId: "s" } },
      askThreads: {},
      askScopePreference: null,
      setAskScope: vi.fn(),
      setGroupId: vi.fn(),
      rollbackFailedGroup: vi.fn(),
      startNewChat: vi.fn(),
      selectChat: vi.fn(),
    }),
}));

import { useChatMode } from "./use-chat-mode";

describe("useChatMode shortcuts", () => {
  afterEach(() => {
    cleanup();
    mocks.transitionChatMode.mockClear();
  });

  it.each(["p", "j"])("toggles Ask with ⌘%s", (key) => {
    renderHook(() => useChatMode());

    fireEvent.keyDown(document.body, {
      key,
      code: `Key${key.toUpperCase()}`,
      metaKey: true,
    });

    expect(mocks.transitionChatMode).toHaveBeenCalledWith({ type: "TOGGLE" });
  });

  it("leaves Ctrl+P (previous line) alone on macOS", () => {
    renderHook(() => useChatMode());

    fireEvent.keyDown(document.body, { key: "p", code: "KeyP", ctrlKey: true });

    expect(mocks.transitionChatMode).not.toHaveBeenCalled();
  });
});
