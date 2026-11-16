import { t } from "@lingui/core/macro";

import { toast } from "@anlg/ui/components/ui/toast";

import { getVoiceEnrollmentStatus } from "./api";

import { liveQueryClient } from "~/db";
import { getStoredSettingValues, setSettingValue } from "~/settings/queries";
import { resolveConfigValue } from "~/shared/config";
import { useTabs } from "~/store/zustand/tabs";

export const SPEAKER_SETUP_TOAST_ID = "speaker-setup-prompt";

export type SpeakerSetupPromptDeps = {
  wasShown: () => Promise<boolean>;
  markShown: () => Promise<void>;
  getProfileName: (ownerUserId: string) => Promise<string>;
  isEnrolled: () => Promise<boolean>;
  show: (onOpenSettings: () => void) => void;
  openSettings: () => void;
};

let promptInFlight = false;

/**
 * After a recording finishes, suggests adding a name and voice profile once.
 * Never throws and never blocks the caller beyond a few local reads.
 */
export async function maybeShowSpeakerSetupPrompt(
  ownerUserId: string | null | undefined,
  deps: SpeakerSetupPromptDeps = defaultDeps,
): Promise<boolean> {
  if (promptInFlight) return false;
  promptInFlight = true;
  try {
    if (await deps.wasShown()) return false;
    const [name, enrolled] = await Promise.all([
      ownerUserId
        ? deps.getProfileName(ownerUserId).catch(() => "")
        : Promise.resolve(""),
      deps.isEnrolled().catch(() => false),
    ]);
    if (name.trim() && enrolled) {
      // Nothing to suggest; never ask later either.
      await deps.markShown();
      return false;
    }
    await deps.markShown();
    deps.show(deps.openSettings);
    return true;
  } catch (error) {
    console.warn("[speakers] setup prompt skipped", error);
    return false;
  } finally {
    promptInFlight = false;
  }
}

const defaultDeps: SpeakerSetupPromptDeps = {
  wasShown: async () =>
    resolveConfigValue(
      "speaker_setup_prompt_shown",
      await getStoredSettingValues(),
    ) === true,
  markShown: () => setSettingValue("speaker_setup_prompt_shown", true),
  getProfileName: async (ownerUserId) => {
    const rows = await liveQueryClient.execute<{ name: string | null }>(
      "SELECT name FROM humans WHERE id = ? AND deleted_at IS NULL",
      [ownerUserId],
    );
    return rows[0]?.name ?? "";
  },
  isEnrolled: async () => (await getVoiceEnrollmentStatus()).enrolled,
  show: (onOpenSettings) => {
    toast(
      t`Add your name and voice profile so your lines are labelled as you.`,
      {
        id: SPEAKER_SETUP_TOAST_ID,
        duration: 12_000,
        closeButton: true,
        action: { label: t`Open Settings`, onClick: onOpenSettings },
      },
    );
  },
  openSettings: () =>
    useTabs.getState().openNew({ type: "settings", state: { tab: "account" } }),
};
