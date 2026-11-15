import { cleanup, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  transitionChatMode: vi.fn(),
}));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (selector: (state: unknown) => unknown) =>
    selector({
      chatMode: "FloatingClosed",
      transitionChatMode: mocks.transitionChatMode,
      currentTab: null,
    }),
}));

vi.mock("./chat-context", () => ({
  useChatContext: (selector: (state: unknown) => unknown) =>
    selector({
      chatByScope: { general: { groupId: undefined, sessionId: "s" } },
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

  it.each(["k", "j"])("toggles Ask with ⌘%s", (key) => {
    renderHook(() => useChatMode());

    fireEvent.keyDown(document.body, {
      key,
      code: `Key${key.toUpperCase()}`,
      metaKey: true,
      ctrlKey: true,
    });

    expect(mocks.transitionChatMode).toHaveBeenCalledWith({ type: "TOGGLE" });
  });
});
