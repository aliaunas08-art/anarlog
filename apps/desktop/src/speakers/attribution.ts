// Reads the optional `attribution` field Rust adds to rendered transcript
// segments. Older transcripts (and builds before the backend lands) do not
// carry it, so every reader tolerates its absence.

export type SpeakerAttributionSource =
  | "channel"
  | "platform_tag"
  | "diarization"
  | "voiceprint"
  | "manual";

export type SegmentAttribution = {
  source: SpeakerAttributionSource;
  confidence: number | null;
  uncertain: boolean;
};

export function getSegmentAttribution(
  segment: object,
): SegmentAttribution | null {
  const value = (segment as { attribution?: unknown }).attribution;
  if (!value || typeof value !== "object") return null;
  const { source, confidence, uncertain } = value as Record<string, unknown>;
  if (typeof source !== "string") return null;
  return {
    source: source as SpeakerAttributionSource,
    confidence: typeof confidence === "number" ? confidence : null,
    uncertain: uncertain === true,
  };
}

export function isSegmentSpeakerUncertain(segment: object): boolean {
  return getSegmentAttribution(segment)?.uncertain === true;
}

export function segmentAttributionEqual(a: object, b: object): boolean {
  const left = getSegmentAttribution(a);
  const right = getSegmentAttribution(b);
  if (!left || !right) return left === right;
  return (
    left.source === right.source &&
    left.confidence === right.confidence &&
    left.uncertain === right.uncertain
  );
}

/** Channel index Rust uses for a segment key's channel profile. */
export function segmentChannelIndex(
  channel: "DirectMic" | "RemoteParty" | "MixedCapture" | string,
): number {
  return channel === "DirectMic" ? 0 : channel === "RemoteParty" ? 1 : 2;
}
