import { cn, safeParseDate } from "@anlg/utils";

import { FolderPicker } from "../folder-picker";
import { useHasTranscript } from "../shared";
import { OverflowButton } from "./overflow";

import { useAudioPlayer } from "~/audio-player";
import { useNow } from "~/calendar/hooks";
import { useShell } from "~/contexts/shell";
import { SessionShareButton } from "~/session-sharing";
import { useSessionEvent } from "~/session/hooks/useSessionEvent";
import {
  usesWindowsStyleTitleBar,
  useWindowControlsGutter,
  WINDOW_CONTROLS_GUTTER_CLASS,
  WINDOW_CONTROLS_GUTTER_PLUS_32_CLASS,
} from "~/shared/hooks/useWindowControlsGutter";
import type { EditorView } from "~/store/zustand/tabs/schema";
import { useListener } from "~/stt/contexts";

/**
 * Top chrome of the session view: folder, share (once the meeting is over)
 * and overflow. View tabs and recording controls live in the bottom bar
 * (`SessionBottomBar`); the title lives in the editorial header.
 */
export function OuterHeader({
  sessionId,
  currentView,
  standaloneWindow = false,
}: {
  sessionId: string;
  currentView: EditorView;
  standaloneWindow?: boolean;
}) {
  const { leftsidebar } = useShell();
  const sessionMode = useListener((state) => state.getSessionMode(sessionId));
  const sessionEvent = useSessionEvent(sessionId);
  const hasTranscript = useHasTranscript(sessionId);
  const { audioExists } = useAudioPlayer();
  const now = useNow();
  const showWindowControlsGutter = useWindowControlsGutter();
  const showSidebarTimelineHeaderGutter =
    !standaloneWindow && !leftsidebar.expanded && !usesWindowsStyleTitleBar();
  const endedAt = sessionEvent?.ended_at
    ? safeParseDate(sessionEvent.ended_at)
    : null;
  const ended = !!endedAt && endedAt.getTime() <= now.getTime();
  const isRecording =
    sessionMode === "active" || sessionMode === "running_batch";
  const meetingOver = !isRecording && (ended || hasTranscript || audioExists);
  const showShare = meetingOver && sessionMode !== "finalizing";

  return (
    <div
      data-tauri-drag-region
      className={cn([
        "relative flex w-full items-center gap-[2px]",
        // 46px content box centers the 28px controls at 23px, matching the
        // sidebar toggle row (pt-[9px] + size-7).
        "h-12 pb-0.5",
        standaloneWindow &&
          (showWindowControlsGutter ? WINDOW_CONTROLS_GUTTER_CLASS : "pl-2"),
        !standaloneWindow && !showSidebarTimelineHeaderGutter && "pl-2",
        showSidebarTimelineHeaderGutter &&
          (showWindowControlsGutter
            ? WINDOW_CONTROLS_GUTTER_PLUS_32_CLASS
            : "pl-[32px]"),
      ])}
    >
      <div
        data-tauri-drag-region
        data-session-header-spacer
        className="min-h-full min-w-0 flex-1"
      />
      <div
        data-tauri-drag-region
        className="relative z-10 flex shrink-0 items-center pr-1"
      >
        <FolderPicker sessionId={sessionId} align="end" />
        {showShare ? (
          <div className="relative mr-1 ml-1 flex min-w-0 shrink-0 items-center">
            <SessionShareButton
              key={sessionId}
              sessionId={sessionId}
              variant="cta"
            />
          </div>
        ) : null}
        <OverflowButton
          standaloneWindow={standaloneWindow}
          sessionId={sessionId}
          currentView={currentView}
        />
      </div>
    </div>
  );
}
