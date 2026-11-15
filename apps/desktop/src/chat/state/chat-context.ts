import { create } from "zustand";

import type { AskScope, AskThreadKey } from "~/chat/guardrails/scope";
import type { ChatScope } from "~/chat/types";
import { id } from "~/shared/utils";

export type ChatSelection = {
  groupId: string | undefined;
  sessionId: string;
};

export type AskResendRequest = {
  scope: AskScope;
  text: string;
  requestId: string;
};

interface ChatContextState {
  chatByScope: Record<ChatScope, ChatSelection>;
  /**
   * General-scope Ask threads, one per thread key (scope + meeting id). A key
   * with no entry yet starts a fresh, empty thread.
   */
  askThreads: Record<AskThreadKey, ChatSelection>;
  /** The user's explicit Ask scope choice; null = default for the context. */
  askScopePreference: AskScope | null;
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
  setAskScope: (scope: AskScope) => void;
  requestAskResend: (scope: AskScope, text: string) => void;
  clearAskResend: (requestId: string) => void;
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

export const useChatContext = create<State>((set) => ({
  chatByScope: {
    general: createChatSelection(),
    automations: createChatSelection(),
  },
  askThreads: {},
  askScopePreference: null,
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
  setAskScope: (askScopePreference) => set({ askScopePreference }),
  requestAskResend: (scope, text) =>
    set({
      askScopePreference: scope,
      pendingAskResend: { scope, text, requestId: id() },
    }),
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
