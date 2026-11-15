import { describe, expect, it, vi } from "vitest";

import {
  classifyAskQuery,
  classifyAskQueryWithFallback,
  detectRefersToPrior,
  extractMeetingTerms,
  parseClassifierReply,
} from "./classifier";

const terms = extractMeetingTerms([
  "Pilot launch planning",
  "We decided to launch the pilot in Toronto first.",
  "The customer asked whether we have a SOC 2 report.",
  "Priya",
]);

describe("classifyAskQuery", () => {
  it.each([
    ["What is an orange", "unrelated"],
    ["Write me a python function to sort a list", "unrelated"],
    ["Help with my homework", "unrelated"],
    ["What did we decide about deadlines?", "meeting_fact"],
    ["Who owns the partner list?", "meeting_fact"],
    ["Explain SOC 2", "general_needed"],
    ["What is Toronto?", "general_needed"],
    ["Draft the follow-up email", "work_product"],
    ["List the action items", "work_product"],
  ] as const)("meeting scope: %s -> %s", (query, intent) => {
    expect(
      classifyAskQuery(query, "meeting", { meetingTerms: terms }).intent,
    ).toBe(intent);
  });

  it("library scope maps facts to library_fact and still redirects trivia", () => {
    expect(
      classifyAskQuery("What did we decide about pricing?", "library").intent,
    ).toBe("library_fact");
    expect(classifyAskQuery("What is an orange", "library").intent).toBe(
      "unrelated",
    );
  });

  it("general scope never classifies a request as unrelated", () => {
    expect(classifyAskQuery("What is an orange", "general").intent).toBe(
      "general_needed",
    );
  });

  it("detects questions about another meeting in meeting scope", () => {
    const context = {
      meetingTitle: "Pilot launch planning",
      meetingTerms: terms,
      otherMeetingTitles: ["Quarterly budget review", "Meeting"],
    };
    expect(
      classifyAskQuery(
        "What happened in the quarterly budget review?",
        "meeting",
        context,
      ).mentionsOtherMeeting,
    ).toBe(true);
    expect(
      classifyAskQuery(
        "What did we cover in the last meeting?",
        "meeting",
        context,
      ).mentionsOtherMeeting,
    ).toBe(true);
    expect(
      classifyAskQuery(
        "What did we decide in this meeting?",
        "meeting",
        context,
      ).mentionsOtherMeeting,
    ).toBe(false);
  });

  it("flags explicit edit requests only", () => {
    expect(
      classifyAskQuery("Make the summary shorter", "meeting").explicitEdit,
    ).toBe(true);
    expect(
      classifyAskQuery("It's not Torronto, it's Toronto", "meeting")
        .explicitEdit,
    ).toBe(true);
    expect(
      classifyAskQuery("Draft the follow-up email", "meeting").explicitEdit,
    ).toBe(false);
  });
});

describe("detectRefersToPrior", () => {
  it.each([
    ["Make the email shorter", true],
    ["make it more formal", true],
    ["That's too long", true],
    ["Add that to the notes", true],
    ["Make the summary shorter", false],
    ["What is an orange", false],
    ["What did we decide?", false],
  ] as const)("%s -> %s", (query, expected) => {
    expect(detectRefersToPrior(query, true)).toBe(expected);
  });

  it("is false without a prior turn", () => {
    expect(detectRefersToPrior("Make it shorter", false)).toBe(false);
  });
});

describe("classifyAskQueryWithFallback", () => {
  it("only consults the model when rules are ambiguous", async () => {
    const model = vi.fn(async () => "unrelated" as const);
    await classifyAskQueryWithFallback(
      "What is an orange",
      "meeting",
      {},
      model,
    );
    expect(model).not.toHaveBeenCalled();

    const result = await classifyAskQueryWithFallback(
      "Thoughts on the vibe?",
      "meeting",
      {},
      model,
    );
    expect(model).toHaveBeenCalledTimes(1);
    expect(result).toMatchObject({ intent: "unrelated", decidedBy: "model" });
  });

  it("falls back to answering when the model fails", async () => {
    const result = await classifyAskQueryWithFallback(
      "Thoughts on the vibe?",
      "meeting",
      {},
      async () => {
        throw new Error("offline");
      },
    );
    expect(result.intent).toBe("meeting_fact");
    expect(result.decidedBy).toBe("ambiguous");
  });

  it("parses one-word replies", () => {
    expect(parseClassifierReply("Unrelated.")).toBe("unrelated");
    expect(parseClassifierReply("???")).toBeNull();
  });
});
