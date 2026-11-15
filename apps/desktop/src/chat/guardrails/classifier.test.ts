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
    ["What is a pizza?", "unrelated"],
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
    ["shorten that", true],
    ["Rewrite the draft with a friendlier tone", true],
    ["Continue the email", true],
    // Needs a prior object AND an edit/continuation verb.
    ["That's too long", false],
    ["Add that to the notes", false],
    ["what is an orange again?", false],
    ["tell me a joke again", false],
    ["who couldn't make it?", false],
    ["make it happen", false],
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

// Reviewer repro cases (scratchpad cls.ts / cls2.ts).
describe("review repro cases", () => {
  const reviewTerms = new Set([
    "pricing",
    "launch",
    "acme",
    "sarah",
    "chen",
    "q3",
    "budget",
    "roadmap",
    "kubernetes",
  ]);
  const classify = (
    query: string,
    scope: "meeting" | "library",
    hasPriorTurns = false,
  ) =>
    classifyAskQuery(query, scope, {
      meetingTerms: scope === "meeting" ? reviewTerms : new Set(),
      hasPriorTurns,
      meetingTitle: "Pricing review",
    });

  it.each([
    "what is an orange again?",
    "tell me a joke again",
    "write me a python script, make it short",
  ])("off-topic beats follow-up detection: %j", (query) => {
    const result = classify(query, "meeting", true);
    expect(result.intent).toBe("unrelated");
    expect(result.refersToPrior).toBe(false);
  });

  it("'who couldn't make it?' is a meeting question that does not send the draft", () => {
    const result = classify("who couldn't make it?", "meeting", true);
    expect(result.intent).toBe("meeting_fact");
    expect(result.refersToPrior).toBe(false);
  });

  it.each(["What's the status of Project Atlas?", "Who is Dana Smith?"])(
    "library with empty vocabulary never redirects %j by rule",
    (query) => {
      expect(classify(query, "library").intent).not.toBe("unrelated");
      expect(
        classifyAskQuery(query, "library", { libraryHits: 2 }),
      ).toMatchObject({ intent: "library_fact", decidedBy: "rules" });
    },
  );

  it.each([
    ["what is the deadline?", "meeting_fact"],
    ["What's this about?", "meeting_fact"],
    ["What was this meeting about?", "meeting_fact"],
    ["explain that", "meeting_fact"],
    ["what is it", "meeting_fact"],
    ["When is our next meeting?", "meeting_fact"],
    ["What should we cover in the next meeting?", "meeting_fact"],
    ["Did anyone mention the previous call?", "meeting_fact"],
    ["who was there?", "meeting_fact"],
    ["Who spoke the most?", "meeting_fact"],
    ["what were the next steps?", "meeting_fact"],
    ["What are my action items?", "meeting_fact"],
    ["summarise this", "work_product"],
    ["Summarize the meeting", "work_product"],
    ["Give me the key points", "work_product"],
    ["tl;dr", "work_product"],
    ["draft the follow-up email", "work_product"],
    ["Translate the summary into French", "work_product"],
    ["What is Kubernetes?", "general_needed"],
  ] as const)("meeting: %j -> %s by rules", (query, intent) => {
    const result = classify(query, "meeting");
    expect(result.intent).toBe(intent);
    expect(result.decidedBy).toBe("rules");
    expect(result.mentionsOtherMeeting).toBe(false);
  });

  it.each([
    "explain quantum physics to me like I'm five, we need it",
    "how do I cook pasta",
    "Write a cover letter for a job",
    "What's the capital of France?",
    "write me an essay on the french revolution",
    "what's 2+2",
    "what is an orange",
  ])("meeting: %j is unrelated by rules", (query) => {
    expect(classify(query, "meeting")).toMatchObject({
      intent: "unrelated",
      decidedBy: "rules",
    });
  });

  it.each([
    "What did the other meeting decide about pricing?",
    "what did we decide in the last meeting about pricing",
  ])("meeting: %j is about another meeting", (query) => {
    expect(classify(query, "meeting").mentionsOtherMeeting).toBe(true);
  });

  it.each([
    ["What did Sarah say about pricing?", "library_fact"],
    ["How many times did we discuss hiring?", "library_fact"],
    ["give me a list of all decisions", "work_product"],
  ] as const)("library: %j -> %s", (query, intent) => {
    expect(classify(query, "library").intent).toBe(intent);
  });

  it.each([
    "Based on the conversation so far, suggest a few questions I could ask next.",
    "Recap what was discussed in the last 5 minutes of this meeting. Use the transcript timestamps, where the latest one is now.",
    "What action items have come up so far in this meeting?",
    "Draft a follow-up email to the participants",
    "What are my action items from this meeting?",
    "What were the key decisions that have been made?",
  ])("chat chip %j stays in meeting scope", (query) => {
    const result = classify(query, "meeting", true);
    expect(result.intent).not.toBe("unrelated");
    expect(result.mentionsOtherMeeting).toBe(false);
    expect(result.decidedBy).toBe("rules");
  });

  it.each([
    "Make the current summary noticeably shorter than it is now. Keep every decision, owner, deadline, and open question; cut repetition and secondary detail.",
    "Make the current summary more detailed than it is now. Add supporting context, rationale, and examples from the transcript without inventing anything.",
    "Make the summary shorter",
    "rewrite the notes",
    "fix the summary",
  ])("summary edit %j is an explicit edit", (query) => {
    expect(classify(query, "meeting", true).explicitEdit).toBe(true);
  });

  it("a chip's explicit intent forces an edit work product", () => {
    expect(
      classifyAskQuery("anything", "meeting", {
        explicitIntent: "edit_summary",
      }),
    ).toMatchObject({ intent: "work_product", explicitEdit: true });
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
