import { useChatContext } from "./chat-context";

import { useSessionTab } from "~/chat/components/use-session-tab";
import {
  type AskScope,
  type AskThreadKey,
  askThreadKey,
  resolveAskScope,
} from "~/chat/guardrails/scope";

export type AskThread = {
  askScope: AskScope;
  /** The meeting a "meeting" thread is pinned to; undefined otherwise. */
  meetingId: string | undefined;
  /** The open meeting tab, whatever the scope (enables "This meeting"). */
  openMeetingId: string | undefined;
  threadKey: AskThreadKey;
  setAskScope: (scope: AskScope) => void;
};

/** The active Ask thread: scope from the user's choice + the open meeting. */
export function useAskThread(): AskThread {
  const { currentSessionId } = useSessionTab();
  const preference = useChatContext((state) => state.askScopePreference);
  const setAskScope = useChatContext((state) => state.setAskScope);
  const askScope = resolveAskScope(preference, currentSessionId);
  const meetingId = askScope === "meeting" ? currentSessionId : undefined;

  return {
    askScope,
    meetingId,
    openMeetingId: currentSessionId,
    threadKey: askThreadKey(askScope, meetingId),
    setAskScope,
  };
}
