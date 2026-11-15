import { useLingui } from "@lingui/react/macro";
import { useRef } from "react";
import { useHotkeys } from "react-hotkeys-hook";

import { cn } from "@anlg/utils";

import { formatElapsed } from "~/session/bookmark";
import { LevelMeter } from "~/shared/level-meter";
import { primaryHotkey } from "~/shared/primary-hotkey";
import { useListener } from "~/stt/contexts";

/**
 * Recording controls that take Record's place inside the session bottom
 * bar's glass capsule while listening:
 * pulsing dot · elapsed time · live level meter · (audio saved) · pause ·
 * bookmark · Stop.
 * While paused the dot goes gray and still, the timer freezes under a
 * "Paused" label and the meter is hidden.
 */
export function RecordingPill({
  sessionId,
  active,
  disabled = false,
  stopTitle,
  onStop,
  onBookmark,
  audioSaved = false,
}: {
  sessionId: string;
  /** Whether this session tab is the visible one (gates the ⌘B shortcut). */
  active: boolean;
  disabled?: boolean;
  stopTitle?: string;
  onStop: () => void;
  onBookmark?: (elapsedSeconds: number) => void;
  /** The recording's audio is kept on disk after the meeting. */
  audioSaved?: boolean;
}) {
  const { t } = useLingui();
  const { seconds, muted, paused, canPause, setPaused } = useListener(
    (state) => {
      const isLive = state.live.sessionId === sessionId;
      return {
        seconds: isLive ? state.live.seconds : 0,
        muted: isLive && state.live.muted,
        paused: isLive && state.live.paused,
        canPause: isLive && state.live.status === "active",
        setPaused: state.setPaused,
      };
    },
  );
  const secondsRef = useRef(seconds);
  secondsRef.current = seconds;
  const elapsed = formatElapsed(seconds);

  const bookmark = () => onBookmark?.(secondsRef.current);
  const togglePaused = () => {
    if (canPause && !disabled) {
      void setPaused(!paused);
    }
  };

  // ⌘B is bold inside the editor and form fields; the default
  // react-hotkeys-hook options keep it from firing there.
  useHotkeys("mod+b", bookmark, {
    enabled: active && Boolean(onBookmark),
    preventDefault: true,
  });

  // No editor binds ⌘⇧P, so it also works while typing notes.
  useHotkeys(primaryHotkey("shift+p"), togglePaused, {
    enabled: active && canPause && !disabled,
    preventDefault: true,
    enableOnFormTags: true,
    enableOnContentEditable: true,
  });

  return (
    <div
      role="group"
      aria-label={paused ? t`Recording paused` : t`Recording`}
      data-session-recording-pill
      data-paused={paused || undefined}
      className="flex h-9 shrink-0 items-center gap-3 pr-1 pl-3 whitespace-nowrap"
    >
      <span
        aria-hidden="true"
        data-recording-dot
        className={cn([
          "size-[9px] shrink-0",
          paused ? "rounded-full bg-[var(--cf-secondary)]" : "cf-record-dot",
        ])}
      />
      {paused ? (
        <span
          data-recording-paused-label
          className="text-[13px] font-semibold text-[var(--cf-secondary)]"
        >
          {t`Paused`}
        </span>
      ) : null}
      <span
        role="timer"
        aria-label={
          paused
            ? t`Recording paused at ${elapsed}`
            : t`Recording time ${elapsed}`
        }
        className={cn([
          "text-[13px] font-semibold tabular-nums",
          paused ? "text-[var(--cf-secondary)]" : "text-[var(--cf-label)]",
        ])}
      >
        {elapsed}
      </span>
      {paused ? null : (
        <PillLevelMeter
          sessionId={sessionId}
          muted={muted}
          data-recording-level-meter
          className="motion-reduce:hidden @max-[560px]:hidden"
        />
      )}
      {audioSaved ? (
        <span
          role="img"
          data-session-audio-saved
          aria-label={t`Audio saved to this Mac`}
          title={t`Audio saved to this Mac`}
          className="flex shrink-0 items-center text-[var(--cf-secondary)]"
        >
          <svg
            aria-hidden="true"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            className="size-3"
          >
            <rect x="5" y="11" width="14" height="10" rx="2" />
            <path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
        </span>
      ) : null}
      <button
        type="button"
        data-tauri-drag-region="false"
        data-recording-pause-toggle
        aria-label={paused ? t`Resume recording` : t`Pause recording`}
        title={paused ? t`Resume recording (⌘⇧P)` : t`Pause recording (⌘⇧P)`}
        disabled={disabled || !canPause}
        onClick={togglePaused}
        className="cf-motion rounded-pill flex size-7 shrink-0 items-center justify-center bg-[var(--cf-glass-sel)] text-[var(--cf-label)] transition-[filter] [corner-shape:round] hover:brightness-95 focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none disabled:cursor-default disabled:opacity-60"
      >
        {paused ? (
          <svg
            aria-hidden="true"
            data-icon="play"
            viewBox="0 0 24 24"
            fill="currentColor"
            className="size-3.5"
          >
            <path d="M8 5.14v13.72a1 1 0 0 0 1.52.85l10.6-6.86a1 1 0 0 0 0-1.7L9.52 4.29A1 1 0 0 0 8 5.14z" />
          </svg>
        ) : (
          <svg
            aria-hidden="true"
            data-icon="pause"
            viewBox="0 0 24 24"
            fill="currentColor"
            className="size-3.5"
          >
            <rect x="6" y="4.5" width="4" height="15" rx="1" />
            <rect x="14" y="4.5" width="4" height="15" rx="1" />
          </svg>
        )}
      </button>
      {onBookmark ? (
        <button
          type="button"
          data-tauri-drag-region="false"
          aria-label={t`Bookmark this moment`}
          title={t`Bookmark this moment (⌘B)`}
          onClick={bookmark}
          className="cf-motion rounded-pill flex size-7 shrink-0 items-center justify-center bg-[var(--cf-glass-sel)] text-[var(--cf-label)] transition-[filter] [corner-shape:round] hover:brightness-95 focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none"
        >
          <svg
            aria-hidden="true"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinejoin="round"
            className="size-3.5"
          >
            <path d="M6 3h12v18l-6-4-6 4z" />
          </svg>
        </button>
      ) : null}
      <button
        type="button"
        data-tauri-drag-region="false"
        aria-label={t`Stop`}
        title={stopTitle ?? t`Stop listening`}
        disabled={disabled}
        onClick={onStop}
        className="cf-motion rounded-pill inline-flex h-7 shrink-0 items-center bg-[var(--cf-label)] px-3 text-xs font-medium text-[var(--cf-window)] transition-opacity [corner-shape:round] hover:opacity-85 focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-default disabled:opacity-60"
      >
        {t`Stop`}
      </button>
    </div>
  );
}

/**
 * Subscribes to the input level on its own, so amplitude updates (many a
 * second) re-render only the meter, not the whole pill.
 */
function PillLevelMeter({
  sessionId,
  ...props
}: { sessionId: string } & Omit<
  React.ComponentProps<typeof LevelMeter>,
  "level"
>) {
  const { level } = useListener((state) => ({
    level:
      state.live.sessionId === sessionId
        ? Math.min(
            Math.hypot(state.live.amplitude.mic, state.live.amplitude.speaker),
            1,
          )
        : 0,
  }));

  return <LevelMeter level={level} {...props} />;
}
