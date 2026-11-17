import { useMemo } from "react";

import type { CharTask } from "@anlg/api-client";

import {
  createEngineModel,
  ENGINE_MODEL_ID,
  ENGINE_PROVIDER_ID,
  useEngineKeyStatus,
} from "~/ai/engine";
import { useSettingsReady } from "~/settings/queries";
import { useConfigValue } from "~/shared/config";

// Every AI feature gets its model here, and the only model is the
// Efficiency Engine's (ASPC C7). There is no provider or model choice: the
// Engine pins the model in Rust and holds the key.

type LanguageModelV3 = ReturnType<typeof createEngineModel>;

type LLMConnectionInfo = {
  providerId: typeof ENGINE_PROVIDER_ID;
  modelId: string;
};

export type LLMConnectionStatus =
  | { status: "pending"; reason: "managed_disabled" | "settings_loading" }
  | {
      status: "error";
      reason: "missing_config";
      providerId: typeof ENGINE_PROVIDER_ID;
      missing: Array<"api_key">;
    }
  | {
      status: "success";
      providerId: typeof ENGINE_PROVIDER_ID;
      isHosted: false;
    };

type LLMConnectionResult = {
  conn: LLMConnectionInfo | null;
  status: LLMConnectionStatus;
};

const ENGINE_CONN: LLMConnectionInfo = {
  providerId: ENGINE_PROVIDER_ID,
  modelId: ENGINE_MODEL_ID,
};

export const useLanguageModel = (task?: CharTask): LanguageModelV3 | null => {
  const { conn } = useLLMConnection();
  return useMemo(
    () => (conn ? createEngineModel({ task: task ?? null }) : null),
    [conn, task],
  );
};

export const useLLMConnection = (): LLMConnectionResult => {
  const settingsReady = useSettingsReady();
  const intelligenceDisabled = useConfigValue("intelligence_disabled");
  const keyStatus = useEngineKeyStatus();
  // Windows without Keys access (the composer) can't see presence; they
  // still get the model and a missing key surfaces as "AI paused".
  const keyMissing = keyStatus.data?.present === false;

  return useMemo<LLMConnectionResult>(() => {
    if (!settingsReady || intelligenceDisabled) {
      return {
        conn: null,
        status: {
          status: "pending",
          reason: intelligenceDisabled
            ? "managed_disabled"
            : "settings_loading",
        },
      };
    }
    if (keyMissing) {
      return {
        conn: ENGINE_CONN,
        status: {
          status: "error",
          reason: "missing_config",
          providerId: ENGINE_PROVIDER_ID,
          missing: ["api_key"],
        },
      };
    }
    return {
      conn: ENGINE_CONN,
      status: {
        status: "success",
        providerId: ENGINE_PROVIDER_ID,
        isHosted: false,
      },
    };
  }, [settingsReady, intelligenceDisabled, keyMissing]);
};

export const useLLMConnectionStatus = (): LLMConnectionStatus => {
  const { status } = useLLMConnection();
  return status;
};
