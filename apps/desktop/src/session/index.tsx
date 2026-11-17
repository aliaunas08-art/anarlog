import { useQuery } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import React, { useEffect, useRef } from "react";
import { useHotkeys } from "react-hotkeys-hook";

import { commands as auditCommands } from "@anlg/plugin-audit";
import { commands as fsSyncCommands } from "@anlg/plugin-fs-sync";
import { computeCurrentNoteTab } from "@anlg/utils/session";

import { EditorialHeader } from "./components/editorial-header";
import { SessionBottomBar } from "./components/floating";
import { NoteConflictBanner } from "./components/note-conflict-banner";
import {
  NoteInput,
  shouldShowTranscriptTabSpinner,
  type NoteInputHandle,
} from "./components/note-input";
import {
  createEditorTabs,
  SessionViewSwitcher,
} from "./components/note-input/header";
import { SearchProvider } from "./components/note-input/search/context";
import { OuterHeader } from "./components/outer-header";
import { PendingProposalsBanner } from "./components/pending-proposals-banner";
import { SessionActionCapsules } from "./components/session-actions";
import { SessionSurface } from "./components/session-surface";
import { getCanShowTranscript, useHasTranscript } from "./components/shared";
import type { TitleInputHandle } from "./components/title-input";
import { useAutoEnhance } from "./hooks/useAutoEnhance";
import { useEnhancedNotes } from "./hooks/useEnhancedNotes";
import { useSessionBookmark } from "./hooks/useSessionBookmark";
import { shouldShowSessionTopAudioPlayer } from "./top-audio-player";
import { getSessionEvent } from "./utils";

import * as AudioPlayer from "~/audio-player";
import { isLockedFlag } from "~/lock/flag";
import { revealLockedNote } from "~/lock/notes";
import { NoteLockScreen } from "~/lock/screen";
import { useAppLock } from "~/lock/store";
import {
  isCanonicalSessionImportLocked,
  subscribeCanonicalSessionImportLocks,
} from "~/session-sharing/editor-activity";
import { useSession } from "~/session/queries";
import { useSessionSharedNoteIds } from "~/shared-notes/event-keys";
import { type Tab, useTabs } from "~/store/zustand/tabs";
import { useListener } from "~/stt/contexts";
import { ScheduledSessionAutoStart } from "~/stt/scheduled-session-auto-start";

export function TabContentNote({
  standaloneWindow = false,
  tab,
}: {
  standaloneWindow?: boolean;
  tab: Extract<Tab, { type: "sessions" }>;
}) {
  const importLocked = React.useSyncExternalStore(
    subscribeCanonicalSessionImportLocks,
    () => isCanonicalSessionImportLocked(tab.id),
    () => isCanonicalSessionImportLocked(tab.id),
  );

  if (importLocked) return <SessionContentLoading />;

  return <LockedNoteGate tab={tab} standaloneWindow={standaloneWindow} />;
}

function LockedNoteGate({
  standaloneWindow,
  tab,
}: {
  standaloneWindow: boolean;
  tab: Extract<Tab, { type: "sessions" }>;
}) {
  const session = useSession(tab.id);
  const locked = isLockedFlag(session?.locked);
  const revealed = useAppLock((state) =>
    Boolean(state.revealedNoteIds[tab.id]),
  );
  const visible = Boolean(session) && (!locked || revealed);
  // ASPC D1.4 "meeting viewed": once the content is on screen. Rust
  // debounces repeats per meeting.
  useEffect(() => {
    if (!visible) return;
    void auditCommands.recordMeetingView(tab.id).catch(() => undefined);
  }, [visible, tab.id]);
  const authenticating = useAppLock((state) => state.authenticating);
  const lockOverlay =
    session && locked && !revealed ? (
      <NoteLockScreen
        sessionTitle={session.title}
        authenticating={authenticating}
        onUnlock={() => {
          void revealLockedNote(tab.id);
        }}
      />
    ) : null;

  return (
    <UnlockedTabContentNote
      tab={tab}
      standaloneWindow={standaloneWindow}
      lockOverlay={lockOverlay}
    />
  );
}

function UnlockedTabContentNote({
  standaloneWindow,
  tab,
  lockOverlay,
}: {
  standaloneWindow: boolean;
  tab: Extract<Tab, { type: "sessions" }>;
  lockOverlay: React.ReactNode;
}) {
  const sessionMode = useListener((state) => state.getSessionMode(tab.id));
  const audioExists = AudioPlayer.useAudioExists(tab.id);

  const audioUrlQuery = useQuery({
    enabled: sessionMode !== "active" && sessionMode !== "finalizing",
    queryKey: ["audio", tab.id, "url"],
    queryFn: () => fsSyncCommands.audioPath(tab.id),
    select: (result) => {
      if (result.status === "error") {
        return null;
      }
      return convertFileSrc(result.data);
    },
  });
  const audioUrl = audioUrlQuery.data;

  return (
    <>
      {tab.state.autoStart && !standaloneWindow ? (
        // Stay mounted under the lock overlay so a locked session can clear
        // autoStart instead of blocking later scheduled meetings.
        <ScheduledSessionAutoStart
          sessionId={tab.id}
          requiresCalendarEligibility={Boolean(tab.state.scheduledAutoStart)}
        />
      ) : null}
      <SearchProvider>
        <AudioPlayer.Provider sessionId={tab.id} url={audioUrl ?? ""}>
          <TabContentNoteInner
            tab={tab}
            standaloneWindow={standaloneWindow}
            audioUrlReady={Boolean(audioUrl)}
            audioExists={audioExists}
            lockOverlay={lockOverlay}
          />
        </AudioPlayer.Provider>
      </SearchProvider>
    </>
  );
}

function TabContentNoteInner({
  tab,
  standaloneWindow,
  audioUrlReady,
  audioExists,
  lockOverlay,
}: {
  tab: Extract<Tab, { type: "sessions" }>;
  standaloneWindow: boolean;
  audioUrlReady: boolean;
  audioExists: boolean;
  lockOverlay: React.ReactNode;
}) {
  const noteInputRef = React.useRef<NoteInputHandle>(null);
  const titleInputRef = React.useRef<TitleInputHandle>(null);
  const handleBookmark = useSessionBookmark(tab.id, noteInputRef);

  const sessionId = tab.id;
  const [editingTranscriptSessionId, setEditingTranscriptSessionId] =
    React.useState<string | null>(null);
  const transcriptEditMode = editingTranscriptSessionId === sessionId;

  const hasTranscript = useHasTranscript(sessionId);
  const sessionMode = useListener((state) => state.getSessionMode(sessionId));
  const batchError = useListener((state) => state.batch[sessionId]?.error);
  const hasLiveSegments = useListener(
    (state) =>
      state.live.sessionId === sessionId && state.liveSegments.length > 0,
  );
  const canShowTranscript = getCanShowTranscript({
    audioExists,
    batchError: Boolean(batchError),
    hasLiveSegments,
    hasTranscript,
    sessionMode,
  });
  const enhancedNoteIds = useEnhancedNotes(
    sessionId,
    hasTranscript && sessionMode === "inactive",
  );
  const session = useSession(sessionId);
  const sessionEvent = session ? getSessionEvent(session) : null;
  const updateSessionTabState = useTabs((state) => state.updateSessionTabState);

  useAutoEnhance(tab);
  const isTranscribing = shouldShowTranscriptTabSpinner(sessionMode);
  const isLiveSessionActive = sessionMode === "active";
  const sharedNoteIds = useSessionSharedNoteIds(sessionId);
  const editorTabs = React.useMemo(
    () =>
      createEditorTabs({
        enhancedNoteIds,
        canShowTranscript,
        sharedNoteIds,
      }),
    [enhancedNoteIds, canShowTranscript, sharedNoteIds],
  );
  const currentView = React.useMemo(() => {
    return computeCurrentNoteTab(
      tab.state.view ?? null,
      isLiveSessionActive,
      enhancedNoteIds,
      canShowTranscript,
      sharedNoteIds,
    );
  }, [
    tab.state.view,
    isLiveSessionActive,
    enhancedNoteIds,
    canShowTranscript,
    sharedNoteIds,
  ]);
  useAutoFocusEditor({
    sessionId,
    noteInputRef,
    enabled: !lockOverlay,
  });

  const showTopAudioPlayer = shouldShowSessionTopAudioPlayer({
    audioExists,
    audioUrlReady,
    currentView,
    sessionMode,
  });

  const handleTabChange = React.useCallback(
    (view: typeof currentView) => {
      if (view.type !== "transcript") {
        blurActiveTranscriptEditor();
        setEditingTranscriptSessionId(null);
      }
      noteInputRef.current?.prepareForTabChange();
      updateSessionTabState(tab, { ...tab.state, view });
    },
    [tab, updateSessionTabState],
  );
  // The title is set far larger than the body, so caret x-positions don't map
  // between them; moving across lands at the title's end or the note's start.
  const handleNavigateToTitle = React.useCallback(() => {
    titleInputRef.current?.focusAtEnd();
  }, []);
  const handleFocusEditorAtStart = React.useCallback(() => {
    noteInputRef.current?.focusAtStart();
  }, []);
  const handleTransferTitleToEditor = React.useCallback((content: string) => {
    noteInputRef.current?.insertAtStartAndFocus(content);
  }, []);
  const handleTranscriptEditModeChange = React.useCallback(
    (editMode: boolean) => {
      if (!editMode) {
        blurActiveTranscriptEditor();
      }
      setEditingTranscriptSessionId(editMode ? sessionId : null);
    },
    [sessionId],
  );

  useHotkeys(
    "mod+s, escape",
    () => handleTranscriptEditModeChange(false),
    {
      enabled:
        tab.active &&
        !lockOverlay &&
        currentView.type === "transcript" &&
        transcriptEditMode,
      enableOnContentEditable: true,
      enableOnFormTags: true,
      preventDefault: true,
    },
    [handleTranscriptEditModeChange],
  );
  return (
    <>
      <SessionSurface
        overlay={lockOverlay}
        header={
          lockOverlay ? undefined : (
            <OuterHeader
              sessionId={sessionId}
              currentView={currentView}
              standaloneWindow={standaloneWindow}
            />
          )
        }
        floatingButton={
          lockOverlay ? undefined : (
            <SessionBottomBar
              tab={tab}
              onBookmark={standaloneWindow ? undefined : handleBookmark}
              viewSwitcher={
                editorTabs.length > 1 ? (
                  <SessionViewSwitcher
                    sessionId={sessionId}
                    editorTabs={editorTabs}
                    currentTab={currentView}
                    handleTabChange={handleTabChange}
                    isTranscribing={isTranscribing}
                    transcriptEditMode={transcriptEditMode}
                    onTranscriptEditModeChange={handleTranscriptEditModeChange}
                    variant="glass"
                  />
                ) : null
              }
            />
          )
        }
      >
        <div className="flex h-full min-h-0 flex-col">
          {!lockOverlay ? (
            <>
              <NoteConflictBanner sessionId={sessionId} />
              <PendingProposalsBanner sessionId={sessionId} />
            </>
          ) : null}
          {showTopAudioPlayer && !lockOverlay ? (
            <div
              data-session-top-audio-player
              className="shrink-0 px-1 pt-1 pb-2"
            >
              <div className="border-border/70 bg-card/80 overflow-hidden rounded-[22px] border">
                <AudioPlayer.Timeline contentClassName="py-1.5 pr-3 pl-1" />
              </div>
            </div>
          ) : null}
          <div className="min-h-0 flex-1">
            {session ? (
              <NoteInput
                ref={noteInputRef}
                tab={tab}
                rawMd={session.raw_md}
                sessionTitle={session.title}
                eventTitle={sessionEvent?.title}
                eventDescription={sessionEvent?.description}
                editorTabs={editorTabs}
                currentTab={currentView}
                handleTabChange={handleTabChange}
                sessionMode={sessionMode}
                transcriptEditMode={transcriptEditMode}
                onTranscriptEditModeChange={handleTranscriptEditModeChange}
                onNavigateToTitle={
                  currentView.type === "raw" ? handleNavigateToTitle : undefined
                }
                renderLeadingContent={(view) => (
                  <EditorialHeader
                    key={tab.id}
                    ref={titleInputRef}
                    tab={tab}
                    // Summary and shared views carry the title as their own
                    // first-line heading; Transcript has no leading content.
                    showTitle={view.type === "raw"}
                    meta={
                      view.type === "raw"
                        ? "inline"
                        : view.type === "enhanced"
                          ? "editor-title"
                          : "none"
                    }
                    onFocusEditorAtStart={handleFocusEditorAtStart}
                    onTransferContentToEditor={handleTransferTitleToEditor}
                  />
                )}
                renderTrailingContent={(view) =>
                  view.type === "enhanced" ? (
                    <SessionActionCapsules
                      sessionId={sessionId}
                      enhancedNoteId={view.id}
                      onOpenTranscript={
                        editorTabs.some((tab) => tab.type === "transcript")
                          ? () => handleTabChange({ type: "transcript" })
                          : undefined
                      }
                    />
                  ) : null
                }
                hideHeader
              />
            ) : (
              <SessionContentLoading />
            )}
          </div>
        </div>
      </SessionSurface>
    </>
  );
}

function blurActiveTranscriptEditor() {
  const activeElement = document.activeElement;
  if (
    activeElement instanceof HTMLElement &&
    activeElement.matches("[data-transcript-editor]")
  ) {
    activeElement.blur();
  }
}

function SessionContentLoading() {
  return (
    <div className="flex h-full flex-col gap-3 px-4 py-5">
      <div className="bg-muted h-5 w-3/5 animate-pulse rounded-md" />
      <div className="bg-muted/80 h-4 w-4/5 animate-pulse rounded-md" />
      <div className="bg-muted/70 h-4 w-2/3 animate-pulse rounded-md" />
    </div>
  );
}

function useAutoFocusEditor({
  sessionId,
  noteInputRef,
  enabled = true,
}: {
  sessionId: string;
  noteInputRef: React.RefObject<NoteInputHandle | null>;
  enabled?: boolean;
}) {
  const autoFocusedSessionRef = useRef<string | null>(null);
  const sessionReady = useSession(sessionId) != null;

  useEffect(() => {
    if (!enabled || !sessionReady) return;
    if (autoFocusedSessionRef.current === sessionId) return;

    autoFocusedSessionRef.current = sessionId;
    const frame = requestAnimationFrame(() => {
      noteInputRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [enabled, noteInputRef, sessionId, sessionReady]);
}
