import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  appendStored: vi.fn(() => Promise.resolve()),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("~/session/bookmark", async (importOriginal) => ({
  ...(await importOriginal<typeof import("~/session/bookmark")>()),
  appendParagraphToStoredMemo: mocks.appendStored,
}));

vi.mock("@anlg/ui/components/ui/toast", () => ({
  toast: { success: mocks.success, error: mocks.error },
}));

import type { NoteInputHandle } from "../components/note-input";
import { useSessionBookmark } from "./useSessionBookmark";

function noteInput(mounted: boolean) {
  const appendMemoParagraph = vi.fn(() => mounted);
  const ref = {
    current: { appendMemoParagraph } as unknown as NoteInputHandle,
  };
  return { ref, appendMemoParagraph };
}

describe("useSessionBookmark", () => {
  beforeEach(() => {
    mocks.appendStored.mockClear();
    mocks.success.mockClear();
    mocks.error.mockClear();
  });

  it("inserts the bookmark line into the open memo editor", async () => {
    const { ref, appendMemoParagraph } = noteInput(true);
    const { result } = renderHook(() => useSessionBookmark("session-1", ref));

    await result.current(754);

    expect(appendMemoParagraph).toHaveBeenCalledWith("🔖 Bookmark · 12:34");
    expect(mocks.appendStored).not.toHaveBeenCalled();
    expect(mocks.success).toHaveBeenCalled();
  });

  it("writes to the stored memo when the memo editor is not open", async () => {
    const { ref } = noteInput(false);
    const { result } = renderHook(() => useSessionBookmark("session-1", ref));

    await result.current(65);

    expect(mocks.appendStored).toHaveBeenCalledWith(
      "session-1",
      "🔖 Bookmark · 1:05",
    );
    expect(mocks.success).toHaveBeenCalled();
  });

  it("reports a failure instead of losing it silently", async () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => {});
    mocks.appendStored.mockRejectedValueOnce(new Error("busy"));
    const { ref } = noteInput(false);
    const { result } = renderHook(() => useSessionBookmark("session-1", ref));

    await result.current(1);

    expect(mocks.error).toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });
});
