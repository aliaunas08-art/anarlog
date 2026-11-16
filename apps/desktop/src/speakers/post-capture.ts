import {
  attributeSessionSpeakers,
  type SpeakerAttributionMode,
  type SpeakerAttributionResult,
} from "./api";

export const SPEAKER_PASS_TIMEOUT_MS = 180_000;

/**
 * In-person captures record the microphone only, so speakers must be told
 * apart on that channel. A capture recovered after a restart has lost the
 * in-memory in-person flag, so the backend decides from the audio.
 */
export function resolveSpeakerAttributionMode({
  inPerson,
  recovered,
}: {
  inPerson: boolean;
  recovered: boolean;
}): SpeakerAttributionMode {
  if (inPerson) return "in_person";
  return recovered ? "auto" : "online";
}

/**
 * Runs the post-capture speaker pass. Never throws and never waits longer
 * than `timeoutMs`: saving and the summary must not depend on it.
 */
export async function runPostCaptureSpeakerPass({
  sessionId,
  transcriptId,
  mode,
  attribute = attributeSessionSpeakers,
  timeoutMs = SPEAKER_PASS_TIMEOUT_MS,
}: {
  sessionId: string;
  transcriptId: string;
  mode: SpeakerAttributionMode;
  attribute?: typeof attributeSessionSpeakers;
  timeoutMs?: number;
}): Promise<SpeakerAttributionResult | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const result = await Promise.race([
      attribute({ sessionId, transcriptId, mode }),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("speaker pass timed out")),
          timeoutMs,
        );
      }),
    ]);
    console.info("[speakers] attribution pass finished", {
      sessionId,
      mode,
      speakerCount: result?.speakerCount,
      skippedReason: result?.skippedReason,
    });
    return result;
  } catch (error) {
    console.warn("[speakers] attribution pass failed; continuing", {
      sessionId,
      mode,
      error,
    });
    return null;
  } finally {
    clearTimeout(timer);
  }
}
