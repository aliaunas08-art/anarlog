import { getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import type { EditorView } from "prosemirror-view";

import type { JSONContent } from "@anlg/editor/note";

import { flushDatabaseWrites } from "~/db/write-queue";
import { loadSessionRawMd, updateSession } from "~/session/queries/sessions";

/** Elapsed recording time as m:ss, or h:mm:ss past an hour. */
export function formatElapsed(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const seconds = String(safe % 60).padStart(2, "0");

  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

export function appendParagraphToDoc(
  doc: JSONContent,
  text: string,
): JSONContent {
  const blocks = [...(doc.content ?? [])];
  const paragraph: JSONContent = {
    type: "paragraph",
    content: [{ type: "text", text }],
  };
  const last = blocks[blocks.length - 1];
  const lastIsEmptyParagraph =
    last?.type === "paragraph" && !(last.content?.length ?? 0);

  if (lastIsEmptyParagraph) {
    blocks.splice(blocks.length - 1, 0, paragraph);
  } else {
    blocks.push(paragraph);
  }

  return { ...doc, type: "doc", content: blocks };
}

export class BookmarkUnavailableError extends Error {}

/**
 * Appends a paragraph to the stored memo when its editor is not mounted in
 * this view. Refuses (throws) when a standalone note window may be editing
 * the memo, or when the stored body cannot be parsed, rather than risk
 * overwriting it.
 */
export async function appendParagraphToStoredMemo(
  sessionId: string,
  text: string,
): Promise<void> {
  const windows = await getAllWebviewWindows().catch(() => null);
  if (
    !windows ||
    windows.some((window) => window.label === `note-${sessionId}`)
  ) {
    throw new BookmarkUnavailableError("memo is open in another window");
  }

  await flushDatabaseWrites([`session:${sessionId}`]);
  const raw = await loadSessionRawMd(sessionId);
  if (raw === null) {
    throw new BookmarkUnavailableError("session not found");
  }

  let doc: JSONContent = { type: "doc", content: [] };
  if (raw.trim()) {
    const parsed = JSON.parse(raw) as JSONContent;
    if (parsed?.type !== "doc") {
      throw new BookmarkUnavailableError("unexpected memo format");
    }
    doc = parsed;
  }

  await updateSession(sessionId, {
    raw_md: JSON.stringify(appendParagraphToDoc(doc, text)),
  });
}

/** Appends a plain paragraph to a live editor, keeping a trailing empty line last. */
export function appendParagraphToView(
  view: EditorView | null | undefined,
  text: string,
): boolean {
  if (!view || view.isDestroyed) {
    return false;
  }

  const { state } = view;
  const paragraphType = state.schema.nodes.paragraph;
  if (!paragraphType) {
    return false;
  }

  const paragraph = paragraphType.create(null, state.schema.text(text));
  const last = state.doc.lastChild;
  // Keep a trailing empty line (the caret's usual home) after the new line.
  const insertAt =
    last && last.type === paragraphType && last.content.size === 0
      ? state.doc.content.size - last.nodeSize
      : state.doc.content.size;

  view.dispatch(state.tr.insert(insertAt, paragraph));
  return true;
}
