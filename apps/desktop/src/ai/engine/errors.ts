import type { EngineError } from "@anlg/plugin-ai-engine";

// Stable codes from crates/ai-engine/src/error.rs.
export const ENGINE_PAUSE_CODES = [
  "ai.unavailable",
  "ai.quota",
  "ai.no_key",
  "ai.rate_limited",
  "ai.invalid_key",
] as const;

export type EnginePauseCode = (typeof ENGINE_PAUSE_CODES)[number];

export function isPauseCode(code: string): code is EnginePauseCode {
  return (ENGINE_PAUSE_CODES as readonly string[]).includes(code);
}

const PAUSE_REASONS: Record<EnginePauseCode, string> = {
  "ai.unavailable": "the AI service can't be reached right now",
  "ai.quota": "the AI usage limit is reached",
  "ai.no_key": "no API key is set. Add one in Settings → AI",
  "ai.rate_limited": "too many AI requests. Try again in a minute",
  "ai.invalid_key": "the API key was rejected. Check it in Settings → AI",
};

export function pauseMessage(code: string, detail?: string): string {
  const reason = isPauseCode(code)
    ? code === "ai.quota" && detail
      ? detail
      : PAUSE_REASONS[code]
    : (detail ?? "AI is unavailable");
  return `AI paused: ${reason}. Recording, transcripts and notes keep working.`;
}

/**
 * Error thrown by the engine fetch. Not an APICallError, so the AI SDK does
 * not retry it: a pause is reported once, calmly, and the call ends.
 */
export class AiEngineError extends Error {
  readonly code: string;
  readonly isPause: boolean;

  constructor(error: EngineError) {
    const pause = isPauseCode(error.code);
    super(pause ? pauseMessage(error.code, error.message) : error.message);
    this.name = "AI_EngineError";
    this.code = error.code;
    this.isPause = pause;
  }
}

export function isAiEngineError(error: unknown): error is AiEngineError {
  return (
    error instanceof AiEngineError ||
    (error instanceof Error &&
      error.name === "AI_EngineError" &&
      typeof (error as { code?: unknown }).code === "string")
  );
}
