import { Schema } from "prosemirror-model";
import type { EditorView } from "prosemirror-view";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  windows: [] as Array<{ label: string }>,
  rawMd: "" as string | null,
  updateSession: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getAllWebviewWindows: () => Promise.resolve(mocks.windows),
}));

vi.mock("~/db/write-queue", () => ({
  flushDatabaseWrites: () => Promise.resolve(),
}));

vi.mock("~/session/queries/sessions", () => ({
  loadSessionRawMd: () => Promise.resolve(mocks.rawMd),
  updateSession: mocks.updateSession,
}));

import {
  appendParagraphToStoredMemo,
  appendParagraphToView,
  formatElapsed,
} from "./bookmark";

const schema = new Schema({
  nodes: {
    doc: { content: "block+" },
    paragraph: { group: "block", content: "text*" },
    text: {},
  },
});

function fakeView(paragraphs: string[]) {
  const doc = schema.node(
    "doc",
    null,
    paragraphs.map((text) =>
      schema.node("paragraph", null, text ? [schema.text(text)] : []),
    ),
  );
  const insert = vi.fn(() => "tr");
  const dispatch = vi.fn();
  const view = {
    isDestroyed: false,
    state: { schema, doc, tr: { insert } },
    dispatch,
  } as unknown as EditorView;
  return { view, doc, insert, dispatch };
}

describe("formatElapsed", () => {
  it.each([
    [0, "0:00"],
    [65, "1:05"],
    [754, "12:34"],
    [3_725, "1:02:05"],
  ])("formats %i seconds as %s", (seconds, expected) => {
    expect(formatElapsed(seconds)).toBe(expected);
  });
});

describe("appendParagraphToView", () => {
  it("inserts the bookmark before the trailing empty line", () => {
    const { view, doc, insert, dispatch } = fakeView(["Agenda", ""]);

    expect(appendParagraphToView(view, "🔖 Bookmark · 12:34")).toBe(true);

    const [position, node] = insert.mock.calls[0] as unknown as [
      number,
      { textContent: string },
    ];
    expect(position).toBe(doc.content.size - doc.lastChild!.nodeSize);
    expect(node.textContent).toBe("🔖 Bookmark · 12:34");
    expect(dispatch).toHaveBeenCalledWith("tr");
  });

  it("appends at the end when the last line has text", () => {
    const { view, doc, insert } = fakeView(["Agenda"]);

    appendParagraphToView(view, "🔖 Bookmark · 0:05");

    expect((insert.mock.calls[0] as unknown[])[0]).toBe(doc.content.size);
  });

  it("does nothing without a live editor", () => {
    expect(appendParagraphToView(null, "x")).toBe(false);
  });
});

describe("appendParagraphToStoredMemo", () => {
  beforeEach(() => {
    mocks.windows = [];
    mocks.rawMd = "";
    mocks.updateSession.mockClear();
  });

  it("adds the bookmark line to the stored memo", async () => {
    mocks.rawMd = JSON.stringify({
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "Agenda" }] },
      ],
    });

    await appendParagraphToStoredMemo("session-1", "🔖 Bookmark · 1:00");

    expect(mocks.updateSession).toHaveBeenCalledWith("session-1", {
      raw_md: JSON.stringify({
        type: "doc",
        content: [
          { type: "paragraph", content: [{ type: "text", text: "Agenda" }] },
          {
            type: "paragraph",
            content: [{ type: "text", text: "🔖 Bookmark · 1:00" }],
          },
        ],
      }),
    });
  });

  it("refuses while a standalone note window may be editing the memo", async () => {
    mocks.windows = [{ label: "note-session-1" }];

    await expect(
      appendParagraphToStoredMemo("session-1", "🔖 Bookmark · 1:00"),
    ).rejects.toThrow();
    expect(mocks.updateSession).not.toHaveBeenCalled();
  });

  it("refuses rather than overwrite a memo it cannot parse", async () => {
    mocks.rawMd = "not json";

    await expect(
      appendParagraphToStoredMemo("session-1", "🔖 Bookmark · 1:00"),
    ).rejects.toThrow();
    expect(mocks.updateSession).not.toHaveBeenCalled();
  });
});
