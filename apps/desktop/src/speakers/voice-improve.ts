import {
  getVoiceEnrollmentStatus,
  improveVoiceEnrollmentFromSegment,
} from "./api";

import { getStoredSettingValues, setSettingValue } from "~/settings/queries";
import { resolveConfigValue } from "~/shared/config";

/**
 * After the user marks a speaker as themselves, ask whether to use that audio
 * for their voice profile. Offered when a profile exists, or when they have
 * not said no before. Never applied without an explicit yes.
 */
export async function shouldOfferVoiceImprovement(): Promise<boolean> {
  const [enrolled, declined] = await Promise.all([
    getVoiceEnrollmentStatus().then(
      (status) => status.enrolled,
      () => null,
    ),
    getStoredSettingValues()
      .then(
        (stored) =>
          resolveConfigValue("voice_profile_improve_declined", stored) === true,
      )
      .catch(() => false),
  ]);
  // The backend is unavailable: nothing to improve.
  if (enrolled === null) return false;
  return enrolled || !declined;
}

export function declineVoiceImprovement(): Promise<void> {
  return setSettingValue("voice_profile_improve_declined", true);
}

export function improveVoiceProfileFromSegment(input: {
  sessionId: string;
  channel: number;
  startMs: number;
  endMs: number;
}) {
  return improveVoiceEnrollmentFromSegment(input);
}
