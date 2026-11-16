// Camel-cased wrappers over the generated speaker-attribution and
// voice-enrollment commands of the transcription plugin. Errors are thrown
// as the plain string Tauri returns.
import {
  commands as transcriptionCommands,
  type VoiceEnrollmentStatus as GeneratedVoiceEnrollmentStatus,
} from "@anlg/plugin-transcription";

export type SpeakerAttributionMode = "online" | "in_person" | "auto";

export type SpeakerAttributionResult = {
  diarizedChannels: number[];
  speakerCount: number;
  selfMatchScore: number | null;
  selfMatched: boolean;
  uncertainWords: number;
  skippedReason: string | null;
  platformNamedSpeakers: number;
};

export type VoiceEnrollmentStatus = {
  enrolled: boolean;
  createdAtMs: number | null;
  updatedAtMs: number | null;
  speechSeconds: number | null;
};

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: string };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") throw result.error;
  return result.data;
}

function toStatus(raw: GeneratedVoiceEnrollmentStatus): VoiceEnrollmentStatus {
  return {
    enrolled: raw.enrolled,
    createdAtMs: raw.created_at_ms,
    updatedAtMs: raw.updated_at_ms,
    speechSeconds: raw.speech_seconds,
  };
}

export async function attributeSessionSpeakers(input: {
  sessionId: string;
  transcriptId: string;
  mode: SpeakerAttributionMode;
}): Promise<SpeakerAttributionResult> {
  const raw = unwrap(
    await transcriptionCommands.attributeSessionSpeakers(
      input.sessionId,
      input.transcriptId,
      input.mode,
    ),
  );
  return {
    diarizedChannels: raw.diarized_channels,
    speakerCount: raw.speaker_count,
    selfMatchScore: raw.self_match_score,
    selfMatched: raw.self_matched,
    uncertainWords: raw.uncertain_words,
    skippedReason: raw.skipped_reason,
    platformNamedSpeakers: raw.platform_named_speakers ?? 0,
  };
}

export async function getVoiceEnrollmentStatus(): Promise<VoiceEnrollmentStatus> {
  return toStatus(unwrap(await transcriptionCommands.voiceEnrollmentStatus()));
}

export async function startVoiceEnrollment(): Promise<void> {
  unwrap(await transcriptionCommands.startVoiceEnrollment());
}

export async function finishVoiceEnrollment(): Promise<VoiceEnrollmentStatus> {
  return toStatus(unwrap(await transcriptionCommands.finishVoiceEnrollment()));
}

export async function cancelVoiceEnrollment(): Promise<void> {
  unwrap(await transcriptionCommands.cancelVoiceEnrollment());
}

export async function deleteVoiceEnrollment(): Promise<void> {
  unwrap(await transcriptionCommands.deleteVoiceEnrollment());
}

export async function improveVoiceEnrollmentFromSegment(input: {
  sessionId: string;
  channel: number;
  startMs: number;
  endMs: number;
}): Promise<VoiceEnrollmentStatus> {
  return toStatus(
    unwrap(
      await transcriptionCommands.improveVoiceEnrollmentFromSegment(
        input.sessionId,
        input.channel,
        input.startMs,
        input.endMs,
      ),
    ),
  );
}

/** Tauri rejects with a plain string; normalise it for display. */
export function describeSpeakerCommandError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
