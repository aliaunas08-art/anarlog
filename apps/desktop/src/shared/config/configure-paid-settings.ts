import { getStoredSettingValues, setSettingValues } from "~/settings/queries";
import type { SettingValues } from "~/settings/schema";

// Paid plans default transcription to the hosted STT. Language models are
// not configured here: every LLM feature uses the Efficiency Engine (C7).
export async function configurePaidSettings(): Promise<void> {
  const { values } = await getStoredSettingValues();
  const updates: SettingValues = {};

  if (!values.current_stt_provider) {
    updates.current_stt_provider = "anarlog";
    updates.current_stt_model = "cloud";
  }

  await setSettingValues(updates);
}
