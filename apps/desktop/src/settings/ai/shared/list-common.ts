import { modelName } from "./model-id";

export type ModelIgnoreReason =
  | "common_keyword"
  | "old_model"
  | "date_snapshot"
  | "no_tool"
  | "no_text_input"
  | "no_completion"
  | "not_llm"
  | "not_chat_model"
  | "context_too_small";

export type IgnoredModel = { id: string; reasons: ModelIgnoreReason[] };

export type InputModality = "image" | "text";

export type ModelMetadata = {
  input_modalities?: InputModality[];
};

export type ListModelsResult = {
  models: string[];
  ignored: IgnoredModel[];
  metadata: Record<string, ModelMetadata>;
};

export const DEFAULT_RESULT: ListModelsResult = {
  models: [],
  ignored: [],
  metadata: {},
};
const modelPriorityPatterns = [
  /(?:^|\/)gpt-5\.6(?:-sol)?$/,
  /(?:^|\/)gpt-5\.6-terra$/,
  /(?:^|\/)gpt-5\.6-luna$/,
  /(?:^|\/)gpt-5\.5$/,
  /(?:^|\/)chat-latest$/,
  /(?:^|\/)claude-fable-5[-.]1$/,
  /(?:^|\/)claude-opus-5$/,
  /(?:^|\/)claude-sonnet-(?:5|latest)$/,
  /(?:^|\/)claude-fable-5$/,
  /(?:^|\/)gpt-5\.4$/,
  /(?:^|\/)gpt-5\.4-mini$/,
  /(?:^|\/)gpt-5\.4-nano$/,
  /(?:^|\/)claude-opus-4[-.]8$/,
  /(?:^|\/)claude-haiku-4[-.]5(?:-\d{8})?$/,
  /(?:^|\/)gemini-3\.8-flash$/,
  /(?:^|\/)gemini-3\.7-flash$/,
  /(?:^|\/)gemini-3\.6-flash$/,
  /(?:^|\/)gemini-3\.5-flash-lite$/,
  /(?:^|\/)gemini-3\.1-pro-preview$/,
  /(?:^|\/)gemini-3\.5-flash$/,
  /(?:^|\/)gemini-3\.1-flash-lite$/,
  /(?:^|\/)grok-4\.6$/,
  /(?:^|\/)grok-4\.5$/,
  /(?:^|\/)grok-4\.3$/,
  /(?:^|\/)mistral-medium-(?:3[-.]5|2604)/,
  /(?:^|\/)(?:mistral-small-4|mistral-small-latest|mistral-small-2603)/,
  /(?:^|\/)(?:mistral-large-3|mistral-large-2512)/,
  /(?:^|\/)ministral-(?:3|14b-2512|8b-2512|3b-2512)/,
  /(?:^|\/)kimi-k3$/,
  /(?:^|\/)deepseek-v4-(?:pro|flash)$/,
  /(?:^|\/)glm-5\.3$/,
  /(?:^|\/)muse-spark-1\.3$/,
] as const;

export const isDateSnapshot = (id: string): boolean => {
  if (/-\d{4}-\d{2}-\d{2}/.test(id)) return true;
  if (/-\d{8}$/.test(id)) return true;
  if (/-20\d{2}$/.test(id)) return true;
  return false;
};

export const isNonStreamingModel = (id: string): boolean => {
  const name = modelName(id);
  return /^gpt-\d+(?:\.\d+)*-pro(?:$|-)/.test(name);
};

export const removeNonStreamingModels = (
  result: ListModelsResult,
): ListModelsResult => {
  return {
    models: result.models.filter((id) => !isNonStreamingModel(id)),
    ignored: result.ignored.filter(({ id }) => !isNonStreamingModel(id)),
    metadata: Object.fromEntries(
      Object.entries(result.metadata).filter(
        ([id]) => !isNonStreamingModel(id),
      ),
    ),
  };
};

export const isOldModel = (id: string): boolean => {
  const name = modelName(id);
  const dashedName = name.replace(/\./g, "-");

  if (/^gpt-3\.5/.test(name)) return true;
  if (/^gpt-4/.test(name)) return true;
  if (/^gpt-5($|-)/.test(name)) return true;
  if (/^gpt-5\.[1-3]($|-)/.test(name)) return true;
  if (/^(davinci|babbage|curie|ada)(-|$)/.test(name)) return true;
  if (/^claude-(2|3|instant)/.test(dashedName)) return true;
  if (
    /^claude-opus-4($|-)/.test(dashedName) &&
    !/^claude-opus-4-8($|-)/.test(dashedName)
  ) {
    return true;
  }
  if (/^claude-sonnet-4($|-)/.test(dashedName)) return true;
  if (
    /^claude-haiku-4($|-)/.test(dashedName) &&
    !/^claude-haiku-4-5($|-)/.test(dashedName)
  ) {
    return true;
  }
  if (/^gemini-(1|2)(\.|-)/.test(name)) return true;
  // The Gemini 3.0 previews are deprecated in favour of the 3.x GA line.
  if (/^gemini-3-/.test(name)) return true;
  if (/^(open-)?mistral-(7b|nemo)(-|$)/.test(name)) return true;
  if (/^open-mixtral/.test(name)) return true;
  if (/^mistral-large-(24|240|241|2502|2508)/.test(name)) return true;
  if (/^mistral-medium-(2505|2508|3[.-]1)($|-)/.test(name)) return true;
  if (/^mistral-small-(3|2503|2506)($|-)/.test(name)) return true;
  if (/^magistral-(small|medium)-\d{4}($|-)/.test(name)) return true;
  if (/^devstral-/.test(name)) return true;
  if (/^(pixtral|voxtral-mini-2507|mistral-saba|codestral-2501)/.test(name)) {
    return true;
  }
  if (/^ministral-(3b|8b)-24/.test(name)) return true;
  // Retired 2026-05-15; the slugs silently redirect to grok-4.3.
  if (/^grok-[23]($|-)/.test(name)) return true;
  if (/^grok-4($|-)/.test(name)) return true;
  // deepseek-chat / deepseek-reasoner were discontinued 2026-07-24.
  if (/^deepseek-(chat|reasoner)$/.test(name)) return true;
  if (/^deepseek-(v3|r1)/.test(name)) return true;
  if (/^(kimi-latest|kimi-thinking-preview|moonshot-v1)/.test(name))
    return true;
  if (/^kimi-k2($|-|[.p]5($|-))/.test(name)) return true;
  if (/^glm-4/.test(name)) return true;
  if (/^glm-5($|-|[.p]1($|-))/.test(name)) return true;
  if (/^command-r(-plus)?(-|$)/.test(name)) return true;
  if (/^command(-light)?$/.test(name)) return true;
  if (/^c4ai-aya/.test(name)) return true;
  if (/^llama-?3[.-]/.test(name)) return true;
  if (/^llama-?4-/.test(name)) return true;
  if (/^llama-guard/.test(name)) return true;
  if (/^qwen-(3|max|plus|flash|turbo)/.test(name)) return true;
  if (/^qwen(2|3-)/.test(name)) return true;
  if (/^(qwq|qvq)-/.test(name)) return true;
  return false;
};

export const sortModelsByRecency = (models: string[]): string[] => {
  const priority = (model: string) => {
    const normalized = modelName(model);
    const index = modelPriorityPatterns.findIndex((pattern) =>
      pattern.test(normalized),
    );
    return index === -1 ? modelPriorityPatterns.length : index;
  };

  return [...models].sort((a, b) => {
    const priorityDelta = priority(a) - priority(b);
    if (priorityDelta !== 0) {
      return priorityDelta;
    }
    return a.localeCompare(b);
  });
};
