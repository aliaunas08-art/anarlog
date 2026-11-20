import { EditorState, TextSelection } from "prosemirror-state";
import { describe, expect, it } from "vitest";

import { schema } from "./schema";
import { normalizeTitleHeadingDoc, titleHeadingPlugin } from "./title-layout";

describe("normalizeTitleHeadingDoc", () => {
  it("keeps a body paragraph after an empty title", () => {
    const doc = schema.node("doc", null, [schema.node("paragraph")]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toEqual({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "paragraph" },
      ],
    });
  });

  it("converts the first paragraph into a title heading", () => {
    const doc = schema.node("doc", null, [
      schema.node("paragraph", null, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toEqual({
      type: "doc",
      content: [
        {
          type: "heading",
          attrs: { level: 1 },
          content: [{ type: "text", text: "Planning" }],
        },
        {
          type: "paragraph",
          content: [{ type: "text", text: "Follow up" }],
        },
      ],
    });
  });

  it("inserts an empty title heading before non-text blocks", () => {
    const doc = schema.node("doc", null, [
      schema.node("bulletList", null, [
        schema.node("listItem", null, [
          schema.node("paragraph", null, [schema.text("Follow up")]),
        ]),
      ]),
    ]);

    expect(normalizeTitleHeadingDoc(doc).toJSON()).toMatchObject({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "bulletList" },
      ],
    });
  });
});

describe("titleHeadingPlugin", () => {
  it("restores a body paragraph when only an empty title remains", () => {
    const doc = schema.node("doc", null, [
      schema.node("heading", { level: 1 }, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);
    const state = EditorState.create({
      schema,
      doc,
      plugins: [titleHeadingPlugin()],
    });

    const result = state.applyTransaction(
      state.tr.delete(1, state.doc.content.size),
    );

    expect(result.state.doc.toJSON()).toEqual({
      type: "doc",
      content: [
        { type: "heading", attrs: { level: 1 } },
        { type: "paragraph" },
      ],
    });
  });

  it("restores the first block to a title heading after edits", () => {
    const doc = schema.node("doc", null, [
      schema.node("heading", { level: 1 }, [schema.text("Planning")]),
      schema.node("paragraph", null, [schema.text("Follow up")]),
    ]);
    const state = EditorState.create({
      schema,
      doc,
      plugins: [titleHeadingPlugin()],
    });

    const result = state.applyTransaction(
      state.tr.setNodeMarkup(0, schema.nodes.paragraph),
    );

    expect(result.state.doc.firstChild?.type).toBe(schema.nodes.heading);
    expect(result.state.doc.firstChild?.attrs.level).toBe(1);
  });
});

describe("titleHeadingPlugin decorations", () => {
  const doc = schema.node("doc", null, [
    schema.node("heading", { level: 1 }, [schema.text("Planning")]),
    schema.node("paragraph", null, [schema.text("Follow up")]),
  ]);

  function titleClasses(selectionPos: number) {
    const plugin = titleHeadingPlugin();
    let state = EditorState.create({ doc, plugins: [plugin] });
    state = state.apply(
      state.tr.setSelection(TextSelection.create(state.doc, selectionPos)),
    );
    const decorations = plugin.props.decorations?.call(plugin, state);
    return decorations ? decorations.find(0, doc.firstChild!.nodeSize) : [];
  }

  it("marks the title while the caret is in it", () => {
    expect(titleClasses(3)).toHaveLength(1);
  });

  it("drops the mark when the caret is in the body", () => {
    expect(titleClasses(doc.firstChild!.nodeSize + 2)).toHaveLength(0);
  });
});
