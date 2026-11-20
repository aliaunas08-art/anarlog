import { useCallback } from "react";
import { useHotkeys } from "react-hotkeys-hook";

import { startAskThreadPersistence } from "./ask-thread-persistence";
import { selectChatSelection, useChatContext } from "./chat-context";
import { useAskThread } from "./use-ask-thread";

import type { ChatScope } from "~/chat/types";
import { primaryHotkey } from "~/shared/primary-hotkey";
import { useTabs } from "~/store/zustand/tabs";

export function useChatMode() {
  startAskThreadPersistence();
  const mode = useTabs((state) => state.chatMode);
  const transitionChatMode = useTabs((state) => state.transitionChatMode);
  const scope = useTabs(
    (state): ChatScope =>
      state.currentTab?.type === "automations" ? "automations" : "general",
  );

  const askThread = useAskThread();
  // Automations keep their own selection; general chat is one thread per Ask key.
  const askKey = scope === "general" ? askThread.threadKey : undefined;
  const groupId = useChatContext(
    (state) => selectChatSelection(state, scope, askKey).groupId,
  );
  const sessionId = useChatContext(
    (state) => selectChatSelection(state, scope, askKey).sessionId,
  );
  const setScopedGroupId = useChatContext((state) => state.setGroupId);
  const rollbackFailedScopedGroup = useChatContext(
    (state) => state.rollbackFailedGroup,
  );
  const startNewScopedChat = useChatContext((state) => state.startNewChat);
  const selectScopedChat = useChatContext((state) => state.selectChat);

  const setGroupId = useCallback(
    (groupId: string | undefined) => setScopedGroupId(scope, groupId, askKey),
    [askKey, scope, setScopedGroupId],
  );
  const rollbackFailedGroup = useCallback(
    (failedGroupId: string) =>
      rollbackFailedScopedGroup(scope, failedGroupId, askKey),
    [askKey, rollbackFailedScopedGroup, scope],
  );
  const startNewChat = useCallback(
    () => startNewScopedChat(scope, askKey),
    [askKey, scope, startNewScopedChat],
  );
  const selectChat = useCallback(
    (groupId: string) => selectScopedChat(scope, groupId, askKey),
    [askKey, scope, selectScopedChat],
  );

  // ⌘P is the primary Ask shortcut (shown on the bottom bar's Ask button);
  // ⌘J is kept as an alias. Both toggle, so a second press closes the chat.
  // ⌘K belongs to Search (shared/open-note-dialog.tsx).
  useHotkeys(
    `${primaryHotkey("p")}, ${primaryHotkey("j")}`,
    () => {
      transitionChatMode({ type: "TOGGLE" });
    },
    {
      preventDefault: true,
      enableOnFormTags: true,
      enableOnContentEditable: true,
    },
    [transitionChatMode],
  );

  return {
    mode,
    scope,
    sendEvent: transitionChatMode,
    groupId,
    sessionId,
    askScope: askThread.askScope,
    askMeetingId: askThread.meetingId,
    openMeetingId: askThread.openMeetingId,
    askThreadKey: askThread.threadKey,
    setAskScope: askThread.setAskScope,
    setGroupId,
    rollbackFailedGroup,
    startNewChat,
    selectChat,
  };
}
