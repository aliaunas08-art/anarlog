import { create as mutate } from "mutative";
import type { StoreApi } from "zustand";

import {
  commands as listenerCommands,
  type CaptureConfigUpdate,
  type CaptureParams,
} from "@anlg/plugin-transcription";
import type { TranscriptionParams } from "@anlg/plugin-transcription";

import type { BatchActions, BatchState } from "./batch";
import { hasConflictingBatchSession, runBatchSession } from "./general-batch";
import {
  attachLiveSession,
  startLiveSession,
  stopLiveSession,
  updateLiveSessionConfig,
} from "./general-live";
import {
  type GeneralState,
  type SessionMode,
  getLiveStartBlockReason,
  initialGeneralState,
  markLiveStartRequested,
  setLivePaused,
  setLiveState,
} from "./general-shared";
import type {
  BatchPersistCallback,
  LiveTranscriptPersistCallback,
  OnStoppedCallback,
  TranscriptActions,
  TranscriptState,
} from "./transcript";

import { enqueueSessionAudioOperation } from "~/session/audio-operations";

export type { GeneralState, SessionMode } from "./general-shared";

const CAPTURE_PAUSE_TIMED_OUT = "capture pause request timed out";

export type GeneralActions = {
  start: (
    params: CaptureParams,
    options?: {
      handlePersist?: LiveTranscriptPersistCallback;
      onStopped?: OnStoppedCallback;
    },
  ) => Promise<boolean>;
  stop: () => void;
  attachLiveSession: (
    sessionId: string,
    options?: {
      handlePersist?: LiveTranscriptPersistCallback;
      onStopped?: OnStoppedCallback;
    },
  ) => Promise<"attached" | "inactive" | "error">;
  beginCaptureRecoveryFinalization: (sessionId: string) => boolean;
  finishCaptureRecoveryFinalization: (sessionId: string) => void;
  setMuted: (value: boolean) => void;
  /** Pause or resume the active capture. Resolves to the resulting state. */
  setPaused: (value: boolean) => Promise<boolean>;
  setBatchTranscriptionPending: (sessionId: string, pending: boolean) => void;
  setTriggerAppIds: (appIds: string[] | null) => void;
  updateCaptureConfig: (update: CaptureConfigUpdate) => Promise<void>;
  startTranscription: (
    params: TranscriptionParams,
    options?: {
      signal?: AbortSignal;
      handlePersist?: BatchPersistCallback;
      notifyOnCompletion?: boolean;
      recovery?: boolean;
    },
  ) => Promise<void>;
  stopTranscription: (sessionId: string) => Promise<void>;
  canStartLiveSession: (sessionId: string) => boolean;
  getLiveStartError: (sessionId: string) => string | null;
  getSessionMode: (sessionId: string) => SessionMode;
};

export const createGeneralSlice = <
  T extends GeneralState &
    GeneralActions &
    TranscriptState &
    TranscriptActions &
    BatchActions &
    BatchState,
>(
  set: StoreApi<T>["setState"],
  get: StoreApi<T>["getState"],
): GeneralState & GeneralActions => {
  // Only the latest pause/resume request may settle the state; an earlier
  // reply arriving late must not undo a later toggle.
  let pauseRequestSeq = 0;
  return {
    ...initialGeneralState,
    start: async (params: CaptureParams, options) => {
      const targetSessionId = params.session_id;

      if (!targetSessionId) {
        console.error("[listener] 'start' requires a session_id");
        return false;
      }

      return enqueueSessionAudioOperation(targetSessionId, async () => {
        const currentMode = get().getSessionMode(targetSessionId);
        if (currentMode === "running_batch") {
          console.warn(
            `[listener] cannot start live session while batch processing session ${targetSessionId}`,
          );
          return false;
        }

        const blockReason = getLiveStartBlockReason(
          get().live,
          targetSessionId,
        );
        if (blockReason) {
          console.warn(`[listener] cannot start live session: ${blockReason}`);
          return false;
        }

        setLiveState(set, (live) => {
          markLiveStartRequested(live, targetSessionId);
        });

        if (options?.handlePersist) {
          get().setTranscriptPersist(targetSessionId, options.handlePersist);
        }
        if (options?.onStopped) {
          get().setOnStopped(targetSessionId, options.onStopped);
        }

        const started = await startLiveSession(
          set,
          get,
          targetSessionId,
          params,
        );
        if (!started) {
          if (options?.handlePersist) {
            get().setTranscriptPersist(targetSessionId, undefined);
          }
          if (options?.onStopped) {
            get().setOnStopped(targetSessionId, undefined);
          }
        }

        return started;
      });
    },
    stop: () => {
      stopLiveSession(set, get);
    },
    attachLiveSession: async (sessionId, options) => {
      if (!sessionId) {
        return "inactive";
      }

      return attachLiveSession(set, get, sessionId, options);
    },
    beginCaptureRecoveryFinalization: (sessionId) => {
      let started = false;
      setLiveState(set, (live) => {
        if (live.postStopProcessingBySession[sessionId]) {
          return;
        }
        live.postStopProcessingBySession[sessionId] = true;
        started = true;
      });
      return started;
    },
    finishCaptureRecoveryFinalization: (sessionId) => {
      setLiveState(set, (live) => {
        delete live.postStopProcessingBySession[sessionId];
        if (live.sessionId === sessionId && live.status === "inactive") {
          live.loading = false;
          live.sessionId = null;
        }
      });
    },
    setMuted: (value) => {
      set((state) =>
        mutate(state, (draft) => {
          draft.live.muted = value;
          void listenerCommands.setMicMuted(value);
        }),
      );
    },
    setPaused: async (value) => {
      const { sessionId, status, paused } = get().live;
      if (!sessionId || status !== "active") {
        return paused;
      }

      const seq = ++pauseRequestSeq;
      setLiveState(set, (live) => {
        if (live.sessionId === sessionId) {
          setLivePaused(live, value, Date.now());
        }
      });

      let next: boolean | undefined;
      try {
        const result = await listenerCommands.setCapturePaused(
          sessionId,
          value,
        );
        if (result.status === "ok") {
          next = result.data;
        } else if (result.error === CAPTURE_PAUSE_TIMED_OUT) {
          // Still queued natively; the `paused` event reports the outcome.
          next = value;
        } else {
          console.error(
            "[listener] failed to set capture paused",
            result.error,
          );
        }
      } catch (error) {
        console.error("[listener] failed to set capture paused", error);
      }
      if (seq !== pauseRequestSeq) {
        return get().live.paused;
      }
      const settled = next ?? paused;
      setLiveState(set, (live) => {
        if (live.sessionId === sessionId) {
          setLivePaused(live, settled, Date.now());
        }
      });
      return settled;
    },
    setBatchTranscriptionPending: (sessionId, pending) => {
      setLiveState(set, (live) => {
        if (pending) {
          live.batchTranscriptionPendingBySession[sessionId] = true;
        } else {
          delete live.batchTranscriptionPendingBySession[sessionId];
        }
      });
    },
    setTriggerAppIds: (appIds) => {
      setLiveState(set, (live) => {
        live.triggerAppIds = appIds;
      });
    },
    updateCaptureConfig: async (update) => {
      const live = get().live;
      if (live.status !== "active" || live.sessionId !== update.session_id) {
        return;
      }

      await updateLiveSessionConfig({
        session_id: update.session_id,
        languages: update.languages,
        participant_human_ids: update.participant_human_ids ?? [],
        self_human_id: update.self_human_id ?? null,
        speaker_assignments: update.speaker_assignments ?? [],
      });
    },
    startTranscription: async (params, options) => {
      const sessionId = params.session_id;

      if (!sessionId) {
        throw new Error(
          "[listener] startTranscription requires params.session_id",
        );
      }

      const mode = get().getSessionMode(sessionId);
      if (mode === "active" || mode === "finalizing") {
        throw new Error(
          `[listener] cannot start batch processing while session ${sessionId} is live`,
        );
      }

      if (
        mode === "running_batch" &&
        (!get().batch[sessionId]?.recovered ||
          (await hasConflictingBatchSession(params)))
      ) {
        throw new Error(
          `[listener] session ${sessionId} is already processing in batch mode`,
        );
      }

      if (options?.handlePersist) {
        get().setBatchPersist(sessionId, options.handlePersist);
      }

      await runBatchSession(get, sessionId, params, {
        notifyOnCompletion: options?.notifyOnCompletion,
        recovery: options?.recovery,
        signal: options?.signal,
      });
    },
    stopTranscription: async (sessionId) => {
      if (!sessionId) {
        return;
      }

      try {
        const result = await listenerCommands.stopTranscription(sessionId);
        if (result.status === "error") {
          console.error(result.error);
          return;
        }

        if (get().getSessionMode(sessionId) === "running_batch") {
          get().handleBatchStopped(sessionId);
        }
      } catch (error) {
        console.error(error);
      }
    },
    getLiveStartError: (sessionId) => {
      const { live } = get();
      return live.lastErrorSessionId === sessionId ? live.lastError : null;
    },
    canStartLiveSession: (sessionId) => {
      if (!sessionId) {
        return false;
      }

      if (get().getSessionMode(sessionId) === "running_batch") {
        return false;
      }

      return getLiveStartBlockReason(get().live, sessionId) === null;
    },
    getSessionMode: (sessionId) => {
      if (!sessionId) {
        return "inactive";
      }

      const state = get();

      if (state.live.sessionId === sessionId) {
        return state.live.status;
      }

      if (state.live.finalizingBySession[sessionId]) {
        return "finalizing";
      }

      if (state.batch[sessionId] && !state.batch[sessionId].terminalReason) {
        return "running_batch";
      }

      return "inactive";
    },
  };
};
