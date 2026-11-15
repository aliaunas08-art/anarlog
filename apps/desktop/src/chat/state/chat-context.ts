import { create } from "zustand";

import {
  type AskScope,
  type AskThreadKey,
  askThreadKey,
  resolveAskScope,
} from "~/chat/guardrails/scope";
import type { ChatScope } from "~/chat/types";
import { id } from "~/shared/utils";

export type ChatSelection = {
  groupId: string | undefined;
  sessionId: string;
  /** A pre-scope chat opened read-only in General (shown unfiltered). */
  legacy?: boolean;
};

export type AskResendRequest = {
  scope: AskScope;
  text: string;
  requestId: string;
  createdAt: number;
};

/** A re-ask that has not been sent within this window is dropped. */
export const ASK_RESEND_TTL_MS = 15_000;

interface ChatContextState {
  chatByScope: Record<ChatScope, ChatSelection>;
  /**
   * General-scope Ask threads, one per thread key (scope + meeting id). A key
   * with no entry yet starts a fresh, empty thread.
   */
  askThreads: Record<AskThreadKey, ChatSelection>;
  /**
   * The user's explicit Ask scope choice, valid only while the same meeting
   * (or no meeting) is open; on a meeting change the default applies again.
   */
  askScopePreference: AskScope | null;
  askScopePreferenceMeetingId: string | null;
  /** A redirect button asked to re-send a query in another scope. */
  pendingAskResend: AskResendRequest | null;
}

interface ChatContextActions {
  setGroupId: (
    scope: ChatScope,
    groupId: string | undefined,
    askKey?: AskThreadKey,
  ) => void;
  rollbackFailedGroup: (
    scope: ChatScope,
    failedGroupId: string,
    askKey?: AskThreadKey,
  ) => void;
  startNewChat: (scope: ChatScope, askKey?: AskThreadKey) => void;
  selectChat: (
    scope: ChatScope,
    groupId: string,
    askKey?: AskThreadKey,
  ) => void;
  setAskScope: (scope: AskScope, openMeetingId: string | undefined) => void;
  requestAskResend: (
    scope: AskScope,
    text: string,
    openMeetingId: string | undefined,
  ) => void;
  /**
   * Selects `scope` for the given open meeting and returns that thread's chat
   * session id, so a prompt can be queued into the right thread.
   */
  prepareAskThread: (
    scope: AskScope,
    openMeetingId: string | undefined,
  ) => string;
  /** Opens a pre-scope chat in the General thread, shown read-only. */
  openPreviousChat: (
    groupId: string,
    openMeetingId: string | undefined,
  ) => void;
  clearAskResend: (requestId: string) => void;
}

/** The user's choice if it was made for the meeting now open, else null. */
export function effectiveAskPreference(
  state: Pick<
    ChatContextState,
    "askScopePreference" | "askScopePreferenceMeetingId"
  >,
  openMeetingId: string | undefined,
): AskScope | null {
  return state.askScopePreferenceMeetingId === (openMeetingId ?? null)
    ? state.askScopePreference
    : null;
}

/** Deterministic id so a key without a stored thread renders consistently. */
export function freshAskSessionId(askKey: AskThreadKey) {
  return `ask:${askKey}`;
}

export function selectChatSelection(
  state: Pick<ChatContextState, "chatByScope" | "askThreads">,
  scope: ChatScope,
  askKey?: AskThreadKey,
): ChatSelection {
  if (scope === "general" && askKey) {
    return (
      state.askThreads[askKey] ?? {
        groupId: undefined,
        sessionId: freshAskSessionId(askKey),
      }
    );
  }
  return state.chatByScope[scope];
}

type State = ChatContextState & ChatContextActions;

function updateSelection(
  state: State,
  scope: ChatScope,
  askKey: AskThreadKey | undefined,
  next: (current: ChatSelection) => ChatSelection | null,
): Partial<State> | State {
  const current = selectChatSelection(state, scope, askKey);
  const updated = next(current);
  if (updated === null) return state;
  if (scope === "general" && askKey) {
    return { askThreads: { ...state.askThreads, [askKey]: updated } };
  }
  return { chatByScope: { ...state.chatByScope, [scope]: updated } };
}

export const useChatContext = create<State>((set, get) => ({
  chatByScope: {
    general: createChatSelection(),
    automations: createChatSelection(),
  },
  askThreads: {},
  askScopePreference: null,
  askScopePreferenceMeetingId: null,
  pendingAskResend: null,
  setGroupId: (scope, groupId, askKey) =>
    set((state) =>
      updateSelection(state, scope, askKey, (current) => ({
        ...current,
        groupId,
      })),
    ),
  // Compares against the live groupId, not a value captured when the send
  // started — the failure lands after onGroupCreated already updated it.
  rollbackFailedGroup: (scope, failedGroupId, askKey) =>
    set((state) =>
      updateSelection(state, scope, askKey, (current) =>
        current.groupId !== failedGroupId
          ? null
          : { ...current, groupId: undefined },
      ),
    ),
  startNewChat: (scope, askKey) =>
    set((state) =>
      updateSelection(state, scope, askKey, () => createChatSelection()),
    ),
  selectChat: (scope, groupId, askKey) =>
    set((state) =>
      updateSelection(state, scope, askKey, () => ({
        groupId,
        sessionId: groupId,
      })),
    ),
  setAskScope: (askScopePreference, openMeetingId) =>
    set({
      askScopePreference,
      askScopePreferenceMeetingId: openMeetingId ?? null,
    }),
  // One-off: the scope switch lasts until the open meeting changes.
  requestAskResend: (scope, text, openMeetingId) =>
    set({
      askScopePreference: scope,
      askScopePreferenceMeetingId: openMeetingId ?? null,
      pendingAskResend: { scope, text, requestId: id(), createdAt: Date.now() },
    }),
  openPreviousChat: (groupId, openMeetingId) =>
    set((state) => ({
      askScopePreference: "general",
      askScopePreferenceMeetingId: openMeetingId ?? null,
      askThreads: {
        ...state.askThreads,
        general: { groupId, sessionId: groupId, legacy: true },
      },
    })),
  prepareAskThread: (scope, openMeetingId) => {
    set({
      askScopePreference: scope,
      askScopePreferenceMeetingId: openMeetingId ?? null,
    });
    const effective = resolveAskScope(scope, openMeetingId);
    const key = askThreadKey(
      effective,
      effective === "meeting" ? openMeetingId : undefined,
    );
    return selectChatSelection(get(), "general", key).sessionId;
  },
  clearAskResend: (requestId) =>
    set((state) =>
      state.pendingAskResend?.requestId === requestId
        ? { pendingAskResend: null }
        : state,
    ),
}));

function createChatSelection(): ChatSelection {
  return { groupId: undefined, sessionId: id() };
}
