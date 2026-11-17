import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  commands as engineCommands,
  type EngineStatus,
  type Quotas,
} from "@anlg/plugin-ai-engine";

import { AiEngineError } from "./errors";

const STATUS_KEY = ["ai-engine", "status"] as const;
const KEY_STATUS_KEY = ["ai-engine", "key-status"] as const;

function unwrap<T>(
  result:
    | { status: "ok"; data: T }
    | { status: "error"; error: { code: string; message: string } },
): T {
  if (result.status === "error") throw new AiEngineError(result.error);
  return result.data;
}

export function useEngineStatus() {
  return useQuery({
    queryKey: STATUS_KEY,
    queryFn: async () => unwrap<EngineStatus>(await engineCommands.status()),
    refetchInterval: 15_000,
  });
}

/**
 * Whether a key is stored (presence only). Windows without Keys access (the
 * composer) get an error, which callers treat as "unknown".
 */
export function useEngineKeyStatus() {
  return useQuery({
    queryKey: KEY_STATUS_KEY,
    queryFn: async () => unwrap(await engineCommands.keyStatus()),
    retry: false,
    staleTime: 30_000,
  });
}

function useInvalidateEngine() {
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: STATUS_KEY }),
      queryClient.invalidateQueries({ queryKey: KEY_STATUS_KEY }),
    ]);
}

export function useSetEngineKey() {
  const invalidate = useInvalidateEngine();
  return useMutation({
    mutationFn: async (key: string) =>
      unwrap(await engineCommands.setApiKey(key)),
    onSettled: invalidate,
  });
}

export function useClearEngineKey() {
  const invalidate = useInvalidateEngine();
  return useMutation({
    mutationFn: async () => unwrap(await engineCommands.clearApiKey()),
    onSettled: invalidate,
  });
}

export function useSetEngineQuotas() {
  const invalidate = useInvalidateEngine();
  return useMutation({
    mutationFn: async (quotas: Quotas) =>
      unwrap<EngineStatus>(await engineCommands.setQuotas(quotas)),
    onSettled: invalidate,
  });
}
