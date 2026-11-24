import chroma from "chroma-js";
import { type CSSProperties, useMemo } from "react";

import type { SegmentKey, SegmentWord } from "~/stt/live-segment";

export type HighlightSegment = { text: string; isMatch: boolean };

export type SentenceLine = {
  words: SegmentWord[];
  startMs: number;
  endMs: number;
};

type SegmentColorVars = CSSProperties & {
  "--segment-color-light": string;
  "--segment-color-dark": string;
  "--speaker-dot-light": string;
  "--speaker-dot-dark": string;
};

export function groupWordsIntoLines(words: SegmentWord[]): SentenceLine[] {
  if (words.length === 0) return [];

  const lines: SentenceLine[] = [];
  let currentLine: SegmentWord[] = [];

  for (const word of words) {
    currentLine.push(word);
    const text = word.text.trim();
    if (text.endsWith(".") || text.endsWith("?") || text.endsWith("!")) {
      lines.push({
        words: currentLine,
        startMs: currentLine[0]!.start_ms,
        endMs: currentLine[currentLine.length - 1]!.end_ms,
      });
      currentLine = [];
    }
  }

  if (currentLine.length > 0) {
    lines.push({
      words: currentLine,
      startMs: currentLine[0]!.start_ms,
      endMs: currentLine[currentLine.length - 1]!.end_ms,
    });
  }

  return lines;
}

export function getActiveLineIndex(
  words: SegmentWord[],
  offsetMs: number,
  currentMs: number,
): number | null {
  if (currentMs <= 0 || words.length === 0) return null;

  let lineIndex = 0;
  let lineStartMs = words[0]!.start_ms;

  for (let index = 0; index < words.length; index += 1) {
    const word = words[index]!;
    const text = word.text.trim();
    const closesLine =
      text.endsWith(".") ||
      text.endsWith("?") ||
      text.endsWith("!") ||
      index === words.length - 1;

    if (!closesLine) {
      continue;
    }

    const start = offsetMs + lineStartMs;
    const end = offsetMs + word.end_ms;
    if (currentMs >= start && currentMs <= end) {
      return lineIndex;
    }

    lineIndex += 1;
    lineStartMs = words[index + 1]?.start_ms ?? lineStartMs;
  }

  return null;
}

export function getSegmentColor(
  key: SegmentKey,
  mode: "light" | "dark" = "light",
): string {
  let speakerIndex = key.speaker_index ?? 0;
  if (key.speaker_human_id) {
    speakerIndex = 0;
    for (const character of key.speaker_human_id) {
      speakerIndex =
        (Math.imul(speakerIndex, 31) + character.charCodeAt(0)) >>> 0;
    }
  }

  const channelOffset = key.speaker_human_id
    ? 0
    : key.channel === "RemoteParty"
      ? 180
      : 0;
  // Golden-angle spacing keeps consecutive speakers visually distinct.
  const hue = (10 + speakerIndex * 137.508 + channelOffset) % 360;

  return chroma.oklch(mode === "dark" ? 0.72 : 0.55, 0.15, hue).hex();
}

// Apple system colors (light, dark). Red is left out: it means "recording".
const SPEAKER_DOT_PALETTE: ReadonlyArray<readonly [string, string]> = [
  ["#0071E3", "#0A84FF"], // blue
  ["#FF9500", "#FF9F0A"], // orange
  ["#34C759", "#30D158"], // green
  ["#AF52DE", "#BF5AF2"], // purple
  ["#30B0C7", "#40C8E0"], // teal
  ["#FF2D55", "#FF375F"], // pink
  ["#5856D6", "#5E5CE6"], // indigo
  ["#A2845E", "#AC8E68"], // brown
];

export function getSpeakerDotColor(
  key: SegmentKey,
  mode: "light" | "dark" = "light",
): string {
  let index: number;
  if (key.speaker_human_id) {
    index = 0;
    for (const character of key.speaker_human_id) {
      index = (Math.imul(index, 31) + character.charCodeAt(0)) >>> 0;
    }
  } else {
    // Your mic is blue; the first remote speaker is orange.
    index = (key.speaker_index ?? 0) + (key.channel === "RemoteParty" ? 1 : 0);
  }

  const [light, dark] =
    SPEAKER_DOT_PALETTE[index % SPEAKER_DOT_PALETTE.length]!;
  return mode === "dark" ? dark : light;
}

export function getSegmentColorVars(key: SegmentKey): SegmentColorVars {
  return {
    "--segment-color-light": getSegmentColor(key),
    "--segment-color-dark": getSegmentColor(key, "dark"),
    "--speaker-dot-light": getSpeakerDotColor(key),
    "--speaker-dot-dark": getSpeakerDotColor(key, "dark"),
  };
}

export function useSegmentColorVars(key: SegmentKey): SegmentColorVars {
  return useMemo(() => getSegmentColorVars(key), [key]);
}
