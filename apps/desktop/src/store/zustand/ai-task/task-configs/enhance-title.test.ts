import type { TextStreamPart, ToolSet } from "ai";
import { expect, it } from "vitest";

import { withTitleLine } from "./enhance-title";

async function run(deltas: string[]) {
  async function* source(): AsyncIterable<TextStreamPart<ToolSet>> {
    for (const text of deltas) {
      yield { type: "text-delta", id: "1", text } as TextStreamPart<ToolSet>;
    }
  }
  let title: string | undefined;
  let text = "";
  for await (const part of withTitleLine(source(), (t) => (title = t))) {
    if (part.type === "text-delta") text += part.text;
  }
  return { title, text };
}

it("cuts a title line split across deltas", async () => {
  expect(
    await run([
      "<meeting",
      "_title>Launch",
      " plan</meeting_title>",
      "\n",
      "\n# A",
      "\n- b",
    ]),
  ).toEqual({ title: "Launch plan", text: "# A\n- b" });
});

it("passes text through untouched when there is no title line", async () => {
  expect(await run(["# Summary", "\n- point"])).toEqual({
    title: undefined,
    text: "# Summary\n- point",
  });
});

it("gives up on an unclosed title line instead of holding the summary", async () => {
  const long = `<meeting_title>${"x".repeat(700)}`;
  expect(await run([long, " rest"])).toEqual({
    title: undefined,
    text: `${long} rest`,
  });
});
