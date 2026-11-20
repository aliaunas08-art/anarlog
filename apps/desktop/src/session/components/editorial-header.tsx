import { useLingui } from "@lingui/react/macro";
import { forwardRef, useSyncExternalStore } from "react";

import { cn, parseEventInstant, safeParseDate } from "@anlg/utils";

import { getMeetingDisplay } from "./floating/meeting-action";
import {
  SessionMetaRow,
  TitleAttachedMeta,
  useMetaParticipants,
} from "./session-meta";
import { TitleInput, type TitleInputHandle } from "./title-input";

import { useAudioPlayer } from "~/audio-player";
import { getRemoteMeeting } from "~/session/hooks/useRemoteMeeting";
import { useSessionEvent } from "~/session/hooks/useSessionEvent";
import { useSession } from "~/session/queries";
import { useConfigValue } from "~/shared/config";
import type { Tab } from "~/store/zustand/tabs";
import { isSessionMarkedInPerson } from "~/stt/useStartListening";

/**
 * C · Final editorial header: an uppercase eyebrow (date · where · duration)
 * above the large, editable session title.
 */
export const EditorialHeader = forwardRef<
  TitleInputHandle,
  {
    tab: Extract<Tab, { type: "sessions" }>;
    /** False when the view renders the title itself (summary first-line H1). */
    showTitle?: boolean;
    /**
     * Where the participants row goes: under this header's own title
     * ("inline"), over the gap under the summary editor's first-line title
     * ("editor-title"), or nowhere.
     */
    meta?: "inline" | "editor-title" | "none";
    onFocusEditorAtStart?: () => void;
    onTransferContentToEditor?: (content: string) => void;
  }
>(function EditorialHeader(
  {
    tab,
    showTitle = true,
    meta = showTitle ? "inline" : "none",
    onFocusEditorAtStart,
    onTransferContentToEditor,
  },
  ref,
) {
  const eyebrow = useSessionEyebrow(tab.id);
  const participants = useMetaParticipants(tab.id);

  return (
    <header
      data-session-editorial-header
      className={cn([
        // Board: eyebrow, title and meta 10px apart; 26px down to the body.
        "flex min-w-0 flex-col gap-2.5 pt-1",
        showTitle ? "pb-[26px]" : "pb-2.5",
      ])}
    >
      {eyebrow ? (
        <p className="m-0 truncate text-xs font-semibold tracking-[0.06em] whitespace-nowrap text-[var(--cf-secondary)] uppercase">
          {eyebrow}
        </p>
      ) : null}
      {showTitle ? (
        <TitleInput
          ref={ref}
          tab={tab}
          variant="editorial"
          onFocusEditorAtStart={onFocusEditorAtStart}
          onTransferContentToEditor={onTransferContentToEditor}
          onFocusEditorAtPixelWidth={
            onFocusEditorAtStart ? () => onFocusEditorAtStart() : undefined
          }
        />
      ) : null}
      {meta === "inline" ? (
        <SessionMetaRow participants={participants} />
      ) : null}
      {meta === "editor-title" ? (
        <TitleAttachedMeta sessionId={tab.id} />
      ) : null}
    </header>
  );
});

function useSessionEyebrow(sessionId: string) {
  const { t, i18n } = useLingui();
  const session = useSession(sessionId);
  const event = useSessionEvent(sessionId);
  const { timeStore } = useAudioPlayer();
  const timezone = useConfigValue("timezone") || undefined;
  // Subscribe to the total only so playback progress does not re-render.
  const audioSeconds = useSyncExternalStore(
    timeStore.subscribe,
    () => timeStore.getSnapshot().total,
  );

  const startedAt =
    parseEventInstant(event?.started_at) ??
    (session?.created_at ? safeParseDate(session.created_at) : null);
  const parts: string[] = [];

  if (startedAt) {
    parts.push(formatEyebrowDate(startedAt, i18n.locale, timezone));
  }

  const remote = getRemoteMeeting(event?.meeting_link);
  if (remote) {
    parts.push(getMeetingDisplay(remote.type).name);
  } else if (isSessionMarkedInPerson(sessionId)) {
    parts.push(t`In person`);
  }

  const minutes = getDurationMinutes({
    audioSeconds,
    startedAt: event?.started_at,
    endedAt: event?.ended_at,
  });
  if (minutes) {
    parts.push(t`${minutes} min`);
  }

  return parts.join(" · ");
}

export function formatEyebrowDate(
  date: Date,
  locale: string,
  timeZone?: string,
): string {
  const options: Intl.DateTimeFormatOptions = {
    weekday: "long",
    month: "short",
    day: "numeric",
  };
  try {
    return date.toLocaleDateString(locale, { ...options, timeZone });
  } catch {
    // An unknown stored timezone falls back to the system one.
    return date.toLocaleDateString(locale, options);
  }
}

export function getDurationMinutes({
  audioSeconds,
  startedAt,
  endedAt,
}: {
  audioSeconds: number;
  startedAt?: string;
  endedAt?: string;
}): number | null {
  if (audioSeconds > 0) {
    return Math.max(1, Math.round(audioSeconds / 60));
  }

  const start = parseEventInstant(startedAt);
  const end = parseEventInstant(endedAt);
  if (!start || !end) {
    return null;
  }

  const minutes = Math.round((end.getTime() - start.getTime()) / 60_000);
  return minutes > 0 ? minutes : null;
}
