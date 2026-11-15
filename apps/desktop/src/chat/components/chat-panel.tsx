import { type ReactNode, useCallback, useEffect, useState } from "react";

import { cn } from "@anlg/utils";

import { AskScopeControl } from "./ask-scope-control";
import { ChatBody } from "./body";
import { ChatContent } from "./content";
import { ChatSession, type ChatSessionRenderProps } from "./session-provider";
import { ChatToolbarControls } from "./toolbar-controls";
import { useSessionTab } from "./use-session-tab";
import { useHasEditableSummary } from "./use-summary-quick-actions";

import { useLanguageModel } from "~/ai/hooks";
import { useChatAppearance } from "~/chat/hooks/use-chat-appearance";
import {
  ASK_RESEND_TTL_MS,
  selectChatSelection,
  useChatContext,
} from "~/chat/state/chat-context";
import { useAskThread } from "~/chat/state/use-ask-thread";
import { useChatActions } from "~/chat/store/use-chat-actions";
import { chatFloatingPanelClassNames } from "~/chat/surface";
import type { ChatScope } from "~/chat/types";
import { useShell } from "~/contexts/shell";
import { useSessionHasTranscript } from "~/session/queries";
import { useOwnerUserId } from "~/shared/owner-user";
import { folderIdForNewNote, useSidebarNotes } from "~/sidebar/note-filter";
import { isBatchTranscriptionPending } from "~/store/zustand/listener/general-shared";
import { useListener } from "~/stt/contexts";

export function ChatSessionHost({
  children,
}: {
  children: (sessionProps: ChatSessionRenderProps | null) => ReactNode;
}) {
  const { chat } = useShell();
  const [scopes, setScopes] = useState<ChatScope[]>([chat.scope]);
  if (!scopes.includes(chat.scope)) {
    setScopes([...scopes, chat.scope]);
  }

  return scopes.map((scope) => (
    <ScopedChatSession key={scope} scope={scope}>
      {(sessionProps) => (scope === chat.scope ? children(sessionProps) : null)}
    </ScopedChatSession>
  ));
}

function ScopedChatSession({
  scope,
  children,
}: {
  scope: ChatScope;
  children: (sessionProps: ChatSessionRenderProps | null) => ReactNode;
}) {
  const askThread = useAskThread();
  const activeKey = scope === "general" ? askThread.threadKey : scope;
  // A thread that is still answering stays mounted after a key switch so its
  // stream finishes and onFinish persists the reply; it unmounts once idle.
  const [mountedKeys, setMountedKeys] = useState<string[]>([activeKey]);
  if (!mountedKeys.includes(activeKey)) {
    setMountedKeys([...mountedKeys, activeKey]);
  }
  const releaseKey = useCallback((key: string) => {
    setMountedKeys((keys) => keys.filter((candidate) => candidate !== key));
  }, []);

  return mountedKeys.map((key) => (
    <KeyedChatSession
      key={key}
      scope={scope}
      threadKey={key}
      active={key === activeKey}
      onIdle={releaseKey}
    >
      {children}
    </KeyedChatSession>
  ));
}

function KeyedChatSession({
  scope,
  threadKey,
  active,
  onIdle,
  children,
}: {
  scope: ChatScope;
  threadKey: string;
  active: boolean;
  onIdle: (key: string) => void;
  children: (sessionProps: ChatSessionRenderProps | null) => ReactNode;
}) {
  const askKey = scope === "general" ? threadKey : undefined;
  const groupId = useChatContext(
    (state) => selectChatSelection(state, scope, askKey).groupId,
  );
  const sessionId = useChatContext(
    (state) => selectChatSelection(state, scope, askKey).sessionId,
  );
  const legacy = useChatContext(
    (state) => selectChatSelection(state, scope, askKey).legacy === true,
  );
  const noteFilter = useSidebarNotes((state) => state.noteFilter);
  const folderFilter = useSidebarNotes((state) => state.folderFilter);
  // Only a "This meeting" thread is tied to a meeting: the one in its key.
  const contextSessionId = askKey?.startsWith("meeting:")
    ? askKey.slice("meeting:".length)
    : undefined;
  const folderId =
    scope === "automations"
      ? undefined
      : folderIdForNewNote(noteFilter, folderFilter);
  const ownerUserId = useOwnerUserId();
  const hasAvailableTranscript = useSessionHasTranscript(
    contextSessionId ?? "",
  );
  const batchTranscriptionPending = useListener((state) => {
    if (!contextSessionId) {
      return false;
    }
    return isBatchTranscriptionPending(
      state.getSessionMode(contextSessionId),
      state.live,
      state.live.batchTranscriptionPendingBySession[contextSessionId],
    );
  });

  if (!ownerUserId) {
    return <>{active ? children(null) : null}</>;
  }

  return (
    <ChatSession
      sessionId={sessionId}
      chatGroupId={groupId}
      // A pre-scope chat is shown unfiltered; its turns still never reach the
      // model (the transport only sends turns with this thread's key).
      askThreadKey={legacy ? undefined : askKey}
      currentSessionId={contextSessionId}
      folderId={folderId}
      hasAvailableTranscript={hasAvailableTranscript}
      isBatchTranscriptionPending={batchTranscriptionPending}
      unstyled
    >
      {(sessionProps) =>
        active ? (
          children(sessionProps)
        ) : (
          <IdleRelease
            status={sessionProps.status}
            onIdle={() => onIdle(threadKey)}
          />
        )
      }
    </ChatSession>
  );
}

function IdleRelease({
  status,
  onIdle,
}: {
  status: ChatSessionRenderProps["status"];
  onIdle: () => void;
}) {
  useEffect(() => {
    if (status === "ready" || status === "error") {
      onIdle();
    }
  }, [status, onIdle]);
  return null;
}

export function ChatPanelFrame({
  layout = "floating",
  onDraftContentChange,
  onOpenFloating,
  onOpenRightPanel,
  sessionProps,
}: {
  layout?: "floating" | "right-panel";
  onDraftContentChange?: (hasDraftContent: boolean) => void;
  onOpenFloating?: () => void;
  onOpenRightPanel?: () => void;
  sessionProps: ChatSessionRenderProps | null;
}) {
  const { chat } = useShell();
  const { groupId, setGroupId, rollbackFailedGroup } = chat;
  const { panelClassName, toolbarSurface } = useChatAppearance();
  const isFloating = layout === "floating";
  const model = useLanguageModel("chat");
  const { currentSessionId, currentNoteView } = useSessionTab();
  const askScope = chat.scope === "general" ? chat.askScope : undefined;
  // Meeting-specific suggestions only make sense in a "This meeting" thread.
  const meetingSuggestionsSessionId =
    askScope === "meeting" ? currentSessionId : undefined;
  const hasSummary = useHasEditableSummary(
    meetingSuggestionsSessionId,
    currentNoteView,
  );
  const isLiveMeeting = useListener(
    (state) =>
      !!meetingSuggestionsSessionId &&
      state.getSessionMode(meetingSuggestionsSessionId) === "active",
  );
  const pendingAskResend = useChatContext((state) => state.pendingAskResend);
  const clearAskResend = useChatContext((state) => state.clearAskResend);
  const requestAskResend = useChatContext((state) => state.requestAskResend);
  const openPreviousChat = useChatContext((state) => state.openPreviousChat);

  const handleGroupCreated = useCallback(
    (newGroupId: string) => {
      setGroupId(newGroupId);
    },
    [setGroupId],
  );

  const handleGroupCreateFailed = useCallback(
    (failedGroupId: string) => {
      rollbackFailedGroup(failedGroupId);
    },
    [rollbackFailedGroup],
  );

  const { handleSendMessage } = useChatActions({
    chatScope: chat.scope,
    askScope,
    askMeetingId: chat.askMeetingId,
    groupId,
    onGroupCreated: handleGroupCreated,
    onGroupCreateFailed: handleGroupCreateFailed,
  });

  // A redirect button re-asks the same text once the target scope's own
  // thread is mounted.
  useEffect(() => {
    if (!pendingAskResend) return;
    // A re-ask belongs to the scope it was requested for; drop it if the
    // thread moved elsewhere or it went stale.
    if (
      askScope !== pendingAskResend.scope ||
      Date.now() - pendingAskResend.createdAt > ASK_RESEND_TTL_MS
    ) {
      clearAskResend(pendingAskResend.requestId);
      return;
    }
    if (
      !sessionProps ||
      sessionProps.sessionId !== chat.sessionId ||
      !sessionProps.isSystemPromptReady ||
      !model
    ) {
      return;
    }
    clearAskResend(pendingAskResend.requestId);
    const text = pendingAskResend.text;
    handleSendMessage(
      text,
      [{ type: "text", text }],
      sessionProps.sendMessage,
      [],
    );
  }, [
    askScope,
    chat.sessionId,
    clearAskResend,
    handleSendMessage,
    model,
    pendingAskResend,
    sessionProps,
  ]);

  return (
    <div
      className={cn([
        "flex min-h-0 flex-col overflow-hidden",
        isFloating ? "max-h-full" : "h-full",
        isFloating ? chatFloatingPanelClassNames() : panelClassName,
      ])}
    >
      {chat.scope === "automations" ? null : (
        <div
          data-tauri-drag-region={!isFloating || undefined}
          className={cn([
            "flex shrink-0 pr-0 pl-0",
            isFloating ? "h-11 items-center" : "h-9 items-start pt-[9px]",
          ])}
        >
          <ChatToolbarControls
            chatScope={chat.scope}
            askThreadKey={chat.askThreadKey}
            onSelectPreviousChat={(previousGroupId) =>
              openPreviousChat(previousGroupId, chat.openMeetingId)
            }
            currentChatGroupId={groupId}
            layout={layout}
            onClose={() => chat.sendEvent({ type: "CLOSE" })}
            onNewChat={chat.startNewChat}
            onOpenFloating={onOpenFloating}
            onOpenRightPanel={onOpenRightPanel}
            onSelectChat={chat.selectChat}
            surface={toolbarSurface}
          />
        </div>
      )}
      {askScope ? (
        <div
          data-tauri-drag-region={!isFloating || undefined}
          className={cn([
            "flex shrink-0 pb-1.5",
            isFloating ? "px-3" : "pr-1 pl-3",
          ])}
        >
          <AskScopeControl
            value={askScope}
            meetingAvailable={Boolean(chat.openMeetingId)}
            onChange={chat.setAskScope}
          />
        </div>
      ) : null}
      {sessionProps && (
        <ChatContent
          {...sessionProps}
          layout={layout}
          onDraftContentChange={onDraftContentChange}
          model={model}
          handleSendMessage={handleSendMessage}
        >
          <ChatBody
            messages={sessionProps.messages}
            status={sessionProps.status}
            error={sessionProps.error}
            onReload={sessionProps.regenerate}
            isModelConfigured={!!model}
            hasContext={sessionProps.contextEntities.length > 0}
            isLiveMeeting={isLiveMeeting}
            hasSummary={hasSummary && sessionProps.contextEntities.length > 0}
            askScope={askScope}
            onAskRedirect={(scope, text) =>
              requestAskResend(scope, text, chat.openMeetingId)
            }
            onSendMessage={(content, parts, _contextRefs, options) => {
              handleSendMessage(
                content,
                parts,
                sessionProps.sendMessage,
                sessionProps.pendingRefs,
                options,
              );
            }}
          />
        </ChatContent>
      )}
    </div>
  );
}
