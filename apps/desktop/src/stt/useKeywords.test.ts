import { beforeEach, describe, expect, it, vi } from "vitest";

const execute = vi.hoisted(() => vi.fn());

vi.mock("~/db", () => ({
  liveQueryClient: { execute },
  useLiveQuery: vi.fn(() => ({ data: undefined })),
}));

import {
  formatDictionaryTerms,
  normalizeKeywordList,
  parseDictionaryTermsJson,
  parseDictionaryTermsText,
} from "./keywords";
import {
  buildKeywords,
  buildKeywordSourceText,
  extractHashtagKeywords,
  getSessionKeywords,
} from "./useKeywords";

beforeEach(() => {
  execute.mockReset();
  execute.mockResolvedValue([]);
});

describe("extractHashtagKeywords", () => {
  it.each([
    { input: "This is #awesome and #cool stuff", expected: ["awesome", "cool"] },
    { input: "#日本語 #한글 #Español", expected: ["日本語", "한글", "Español"] },
    {
      input: "Use `#notthis` and\n```\n#norThis\n```\nbut #this",
      expected: ["this"],
    },
    { input: "#a #bb", expected: ["bb"] },
    { input: "", expected: [] },
  ])("$input", ({ input, expected }) => {
    expect(extractHashtagKeywords(input)).toEqual(expected);
  });

  it("no longer extracts words from plain prose", () => {
    expect(
      extractHashtagKeywords(
        "Artificial intelligence and machine learning are transforming technology",
      ),
    ).toEqual([]);
  });
});

describe("buildKeywordSourceText", () => {
  it("includes session note, title, and event metadata", () => {
    expect(
      buildKeywordSourceText({
        rawMd: "Discuss product launch",
        title: "Erebor sync",
        eventJson: JSON.stringify({
          title: "OpenWorld review",
          description: "Airborne Brothers follow-up",
          location: "Zoom",
        }),
      }),
    ).toBe(
      [
        "Discuss product launch",
        "Erebor sync",
        "OpenWorld review",
        "Airborne Brothers follow-up",
        "Zoom",
      ].join("\n"),
    );
  });
});

describe("getSessionKeywords", () => {
  it("builds keywords from the current session snapshot", async () => {
    execute.mockResolvedValue([
      {
        raw_md: "Discuss #Launch and production systems",
        title: "Erebor sync",
        event_json: JSON.stringify({
          title: "OpenWorld review",
          description: "Airborne Brothers follow-up",
          location: "Zoom",
        }),
        participant_names_json: "[]",
        event_participants_json: "[]",
      },
    ]);

    await expect(
      getSessionKeywords({
        sessionId: "session-1",
        dictionaryTerms: ["Anarlog"],
      }),
    ).resolves.toEqual(expect.arrayContaining(["Anarlog", "Launch"]));
  });

  it("prioritizes mapped participants and attached event attendees", async () => {
    execute.mockResolvedValue([
      {
        raw_md: "Discuss #Launch and production systems",
        title: "Erebor sync",
        event_json: JSON.stringify({
          title: "OpenWorld review",
          description: "Airborne Brothers follow-up",
          location: "Zoom",
        }),
        participant_names_json: JSON.stringify(["Alice Kim"]),
        event_participants_json: JSON.stringify([
          { name: "Alice Kim", email: "alice@example.com" },
          { name: "Mina Park", email: "mina@example.com" },
          {
            name: "John Jeong",
            email: "john@example.com",
            is_current_user: true,
          },
        ]),
      },
    ]);

    const result = await getSessionKeywords({
      sessionId: "session-1",
      dictionaryTerms: ["Anarlog"],
    });

    expect(result.slice(0, 3)).toEqual(["Alice Kim", "Mina Park", "Anarlog"]);
    expect(result).toEqual(expect.arrayContaining(["Launch"]));
    expect(result).not.toContain("John Jeong");
  });
});

describe("buildKeywords", () => {
  it("dedupes higher-priority hints and caps the result", () => {
    const result = buildKeywords({
      rawMd: "",
      title: "",
      eventJson: "",
      sessionParticipantTerms: ["Alice Kim"],
      eventParticipantTerms: ["alice kim", "Mina Park"],
      dictionaryTerms: Array.from(
        { length: 60 },
        (_, index) => `Term ${index}`,
      ),
    });

    expect(result).toHaveLength(50);
    expect(result.slice(0, 4)).toEqual([
      "Alice Kim",
      "Mina Park",
      "Term 0",
      "Term 1",
    ]);
    expect(result).not.toContain("alice kim");
  });
});

describe("dictionary term helpers", () => {
  it.each([
    {
      name: "parses stored JSON dictionary terms",
      run: () => parseDictionaryTermsJson(JSON.stringify(["Anarlog", "Char"])),
      expected: ["Anarlog", "Char"],
    },
    {
      name: "parses an array payload and drops duplicate spellings",
      run: () => parseDictionaryTermsJson(["Anarlog", " anarlog "]),
      expected: ["Anarlog"],
    },
    {
      name: "returns no terms for malformed JSON",
      run: () => parseDictionaryTermsJson("not-json"),
      expected: [],
    },
    {
      name: "parses newline and comma separated terms",
      run: () =>
        parseDictionaryTermsText("Anarlog\nFastConformer, Parakeet TDT"),
      expected: ["Anarlog", "FastConformer", "Parakeet TDT"],
    },
    {
      name: "normalizes duplicate terms while preserving first spelling",
      run: () => normalizeKeywordList(["Anarlog", " anarlog ", "Parakeet"]),
      expected: ["Anarlog", "Parakeet"],
    },
  ])("$name", ({ run, expected }) => {
    expect(run()).toEqual(expected);
  });

  it("formats stored terms one per line", () => {
    expect(formatDictionaryTerms(["Anarlog", "Parakeet TDT"])).toBe(
      "Anarlog\nParakeet TDT",
    );
  });
});
