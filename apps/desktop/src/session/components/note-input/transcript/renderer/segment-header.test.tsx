import { cleanup, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SegmentHeader } from "./segment-header";

import type { Segment } from "~/stt/live-segment";

vi.mock("./speaker-assign", () => ({
  SpeakerAssignPopover: ({
    label,
    color,
    title,
  }: {
    label: string;
    color: string;
    title?: string;
  }) => (
    <button type="button" title={title} style={{ color }}>
      {label}
    </button>
  ),
}));

beforeEach(() => {
  cleanup();
});

describe("SegmentHeader", () => {
  it("keeps the speaker label visible without exposing timestamps", () => {
    render(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Speaker 3"
        segment={createRemoteSegment(2)}
      />,
    );

    expect(screen.getByRole("button", { name: "Speaker 3" })).toBeTruthy();
    expect(screen.queryByText("00:12 - 00:18")).toBeNull();
  });

  it("labels remote live segments as the unique other participant", () => {
    render(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Artem"
        segment={createRemoteSegment(0)}
      />,
    );

    expect(screen.getByRole("button", { name: "Artem" })).toBeTruthy();
  });

  it("updates cached remote labels when session participants change", () => {
    const segment = createRemoteSegment(0);
    const { rerender } = render(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Artem"
        segment={segment}
      />,
    );

    expect(screen.getByRole("button", { name: "Artem" })).toBeTruthy();

    rerender(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Speaker 1"
        segment={segment}
      />,
    );

    expect(screen.getByRole("button", { name: "Speaker 1" })).toBeTruthy();
  });
});

describe("SegmentHeader attribution", () => {
  it("mutes uncertain speaker labels and explains them in a tooltip", () => {
    render(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Speaker A"
        segment={
          {
            ...createRemoteSegment(0),
            attribution: {
              source: "diarization",
              confidence: 0.4,
              uncertain: true,
            },
          } as unknown as Segment
        }
      />,
    );

    const label = screen.getByRole("button", { name: "Speaker A" });
    expect(label.getAttribute("title")).toBe("Speaker uncertain");
    expect(label.style.color).toBe("var(--cf-tertiary)");
  });

  it("keeps confident and legacy labels in the secondary colour", () => {
    render(
      <SegmentHeader
        transcriptId="transcript-1"
        sessionId="session-1"
        label="Them"
        segment={createRemoteSegment(0)}
      />,
    );

    const label = screen.getByRole("button", { name: "Them" });
    expect(label.getAttribute("title")).toBeNull();
    expect(label.style.color).toBe("var(--cf-secondary)");
  });
});

function createRemoteSegment(speakerIndex: number): Segment {
  return {
    id: "segment-1",
    key: {
      channel: "RemoteParty",
      speaker_index: speakerIndex,
      speaker_human_id: null,
    },
    start_ms: 12_000,
    end_ms: 18_000,
    text: "hello world",
    words: [
      {
        id: "word-1",
        text: "hello",
        start_ms: 12_000,
        end_ms: 13_000,
        channel: "RemoteParty",
        is_final: true,
      },
    ],
  } as Segment;
}
