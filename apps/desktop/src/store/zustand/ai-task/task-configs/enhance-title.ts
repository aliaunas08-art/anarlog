import type { TextStreamPart, ToolSet } from "ai";

// One call per meeting (ASPC C7.5): the enhance call returns the title on
// its first line as `<meeting_title>…</meeting_title>` (see
// crates/template-app/assets/enhance.system.md.jinja), followed by the
// summary. The line is cut from the stream here, so the summary that is
// shown and saved never contains it, and no separate title call is made.

export const TITLE_OPEN = "<meeting_title>";
export const TITLE_CLOSE = "</meeting_title>";
// If the closing tag hasn't arrived by now, the model ignored the format:
// give the text back untouched rather than hold the summary.
const MAX_TITLE_LINE = 600;

const titles = new WeakMap<object, string>();

export function rememberGeneratedTitle(owner: object, title: string) {
  titles.set(owner, title);
}

/** The title cut from this run's stream, once. */
export function takeGeneratedTitle(owner: object): string | undefined {
  const title = titles.get(owner);
  titles.delete(owner);
  return title;
}

export async function* withTitleLine<TOOLS extends ToolSet>(
  stream: AsyncIterable<TextStreamPart<TOOLS>>,
  onTitle: (title: string) => void,
): AsyncIterable<TextStreamPart<TOOLS>> {
  let head = "";
  let template: TextStreamPart<TOOLS> | null = null;
  let done = false;
  // Whitespace between the title line and the summary may arrive in later
  // deltas; drop it so the summary starts with its first real character.
  let trimLeading = false;

  const textPart = (text: string): TextStreamPart<TOOLS> =>
    ({ ...(template as object), text }) as TextStreamPart<TOOLS>;

  for await (const part of stream) {
    if (done && trimLeading && part.type === "text-delta") {
      const text = part.text.replace(/^\s+/, "");
      if (!text) continue;
      trimLeading = false;
      yield { ...part, text };
      continue;
    }
    if (done || part.type !== "text-delta") {
      yield part;
      continue;
    }
    template ??= part;
    head += part.text;

    const start = head.trimStart();
    const closing = start.indexOf(TITLE_CLOSE);
    if (start.startsWith(TITLE_OPEN) && closing >= 0) {
      onTitle(start.slice(TITLE_OPEN.length, closing).trim());
      const rest = start
        .slice(closing + TITLE_CLOSE.length)
        .replace(/^\s+/, "");
      done = true;
      trimLeading = !rest;
      if (rest) yield textPart(rest);
      continue;
    }
    const mayStillBeTitle =
      start.length < TITLE_OPEN.length
        ? TITLE_OPEN.startsWith(start)
        : start.startsWith(TITLE_OPEN) && head.length <= MAX_TITLE_LINE;
    if (!mayStillBeTitle) {
      done = true;
      yield textPart(head);
    }
  }

  if (!done && head) {
    yield textPart(head);
  }
}
