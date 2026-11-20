import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("~/chat/hooks/use-chat-appearance", () => ({
  useChatAppearance: () => ({
    isDarkAppearance: false,
  }),
}));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: () => vi.fn(),
}));

import { ChatBodyEmpty } from "./empty";

import { classifyAskQuery } from "~/chat/guardrails/classifier";

describe("ChatBodyEmpty", () => {
  beforeEach(() => {
    cleanup();
  });

  it("offers different quick actions for notes, live meetings, and no context", () => {
    const prompts: string[] = [];
    for (const props of [
      { hasContext: true },
      { hasContext: true, isLiveMeeting: true },
      { hasContext: false },
    ]) {
      const onSendMessage = vi.fn();
      const view = render(
        <ChatBodyEmpty {...props} onSendMessage={onSendMessage} />,
      );
      fireEvent.click(screen.getAllByRole("button")[0]);
      const [prompt, parts] = onSendMessage.mock.calls[0];
      expect(prompt.trim().length).toBeGreaterThan(0);
      expect(parts).toEqual([{ type: "text", text: prompt }]);
      prompts.push(prompt);
      view.unmount();
    }
    expect(new Set(prompts).size).toBe(prompts.length);
  });

  it("offers shorter and longer rewrites only when the note has a summary", () => {
    const onSendMessage = vi.fn();
    const view = render(
      <ChatBodyEmpty hasContext hasSummary onSendMessage={onSendMessage} />,
    );
    fireEvent.click(screen.getByText("Make the summary shorter."));
    fireEvent.click(screen.getByText("Make the summary longer."));
    const [shorter, longer] = onSendMessage.mock.calls.map(
      ([prompt]) => prompt,
    );
    expect(shorter.trim()).not.toBe("");
    expect(longer.trim()).not.toBe("");
    expect(shorter).not.toEqual(longer);
    view.unmount();

    render(<ChatBodyEmpty hasContext onSendMessage={vi.fn()} />);
    expect(screen.queryByText("Make the summary shorter.")).toBeNull();
  });

  // Every chip must reach the model in a "This meeting" thread: never a
  // redirect, and the summary chips keep their edit tool via askIntent.
  it.each([
    { label: "Make the summary shorter.", edit: true, live: false },
    { label: "Make the summary longer.", edit: true, live: false },
    { label: "List action items.", edit: false, live: false },
    { label: "Draft follow-up email.", edit: false, live: false },
    { label: "Find key decisions.", edit: false, live: false },
    { label: "Recap the last 5 minutes.", edit: false, live: true },
    { label: "Suggest what to ask next.", edit: false, live: true },
    { label: "List action items so far.", edit: false, live: true },
  ])("chip $label routes in meeting scope", ({ label, edit, live }) => {
    const onSendMessage = vi.fn();
    render(
      <ChatBodyEmpty
        hasContext
        hasSummary
        isLiveMeeting={live}
        askScope="meeting"
        onSendMessage={onSendMessage}
      />,
    );
    fireEvent.click(screen.getByText(label));
    const [prompt, , , options] = onSendMessage.mock.calls[0];
    expect(options?.askIntent).toBe(edit ? "edit_summary" : undefined);
    const classification = classifyAskQuery(prompt, "meeting", {
      hasPriorTurns: true,
      explicitIntent: options?.askIntent,
    });
    expect(classification.intent).not.toBe("unrelated");
    expect(classification.mentionsOtherMeeting).toBe(false);
    expect(classification.decidedBy).toBe("rules");
    if (edit) expect(classification.explicitEdit).toBe(true);
  });
});
