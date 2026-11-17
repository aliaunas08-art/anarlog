import { createAnthropic } from "@ai-sdk/anthropic";
import { wrapLanguageModel } from "ai";

import { createUsageTrackingMiddleware } from "../usage";
import { createEngineFetch, type EngineTransport } from "./transport";

// Mirrors HAIKU_MODEL in crates/ai-engine/src/config.rs (checked by
// engine.test.ts). Informational only: the Engine overwrites the model on
// every request, whatever this says.
export const ENGINE_MODEL_ID = "claude-haiku-5-5";
export const ENGINE_PROVIDER_ID = "engine";

// Not a real host. The engine fetch forwards only the path; Rust decides
// where the request goes (DIRECT: Anthropic, PROXY: the hosted proxy).
const ENGINE_BASE_URL = "https://engine.invalid/v1";
// The real key never enters JS; the SDK just needs a non-empty value.
const PLACEHOLDER_KEY = "engine-managed";

type LanguageModelV3 = Parameters<typeof wrapLanguageModel>[0]["model"];

/**
 * The only language model the app creates. Every AI feature gets this
 * model, which sends each call through the Rust Efficiency Engine.
 */
export function createEngineModel({
  task,
  transport,
}: {
  task?: string | null;
  transport?: EngineTransport;
} = {}): LanguageModelV3 {
  const provider = createAnthropic({
    baseURL: ENGINE_BASE_URL,
    apiKey: PLACEHOLDER_KEY,
    fetch: createEngineFetch(transport),
  });
  return wrapLanguageModel({
    model: provider(ENGINE_MODEL_ID),
    middleware: createUsageTrackingMiddleware({
      task: task ?? null,
      providerId: ENGINE_PROVIDER_ID,
    }),
  });
}
