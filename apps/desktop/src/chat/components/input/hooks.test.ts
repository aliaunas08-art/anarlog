import { describe, expect, it } from "vitest";

import { extractContextRefsFromTiptapJson } from "./hooks";

function docWithMention(type: string, id: string) {
  return {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "find action items in " },
          {
            type: "mention-@",
            attrs: { id, type, label: id },
          },
        ],
      },
    ],
  } as never;
}

describe("extractContextRefsFromTiptapJson", () => {
  it("turns a folder mention into a removable manual folder ref", () => {
    expect(
      extractContextRefsFromTiptapJson(docWithMention("folder", "CS 101")),
    ).toEqual([
      {
        kind: "folder",
        key: "folder:manual:CS 101",
        source: "manual",
        folderId: "CS 101",
      },
    ]);
  });

  it("deduplicates repeated folder mentions", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            {
              type: "mention-@",
              attrs: { id: "CS 101", type: "folder", label: "CS 101" },
            },
            {
              type: "mention-@",
              attrs: { id: "CS 101", type: "folder", label: "CS 101" },
            },
          ],
        },
      ],
    } as never;

    expect(extractContextRefsFromTiptapJson(doc)).toHaveLength(1);
  });
});

describe("usePendingChatPrompt", () => {
  it("puts a queued prompt in the editor and keeps existing text", async () => {
    const { renderHook } = await import("@testing-library/react");
    const { queueChatPrompt, usePendingChatPrompt } = await import("./hooks");
    let doc: { type: string; content?: unknown[] } | undefined = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "draft" }] },
      ],
    };
    const editor = {
      getJSON: () => doc,
      replaceContent: (next: typeof doc) => {
        doc = next;
      },
      focus: () => true,
      clearContent: () => {},
      insertText: () => {},
    };
    renderHook(() =>
      usePendingChatPrompt({
        draftKey: "chat-x",
        editorRef: { current: editor as never },
      }),
    );
    queueChatPrompt("chat-x", "Recap yesterday");
    expect(doc?.content).toEqual([
      { type: "paragraph", content: [{ type: "text", text: "draft" }] },
      {
        type: "paragraph",
        content: [{ type: "text", text: "Recap yesterday" }],
      },
    ]);
  });
});
