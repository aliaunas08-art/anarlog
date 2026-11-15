import { useLingui } from "@lingui/react/macro";
import { useEffect, useRef, useState } from "react";
import { useHotkeys } from "react-hotkeys-hook";

import { cn } from "@anlg/utils";

import { formatElapsed } from "~/session/bookmark";
import { useListener } from "~/stt/contexts";

const METER_BARS = 14;
const METER_SAMPLE_MS = 120;

/** Pauses or resumes the recording from anywhere in the visible session. */
export const PAUSE_RECORDING_HOTKEY = "mod+shift+p";

/**
 * Glass recording pill shown in the session bottom bar while listening:
 * pulsing dot · elapsed time · live level meter · pause · bookmark · Stop.
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
}: {
  sessionId: string;
  /** Whether this session tab is the visible one (gates the ⌘B shortcut). */
  active: boolean;
  disabled?: boolean;
  stopTitle?: string;
  onStop: () => void;
  onBookmark?: (elapsedSeconds: number) => void;
}) {
  const { t } = useLingui();
  const { seconds, level, muted, paused, canPause, setPaused } = useListener(
    (state) => {
      const isLive = state.live.sessionId === sessionId;
      return {
        seconds: isLive ? state.live.seconds : 0,
        level: isLive
          ? Math.min(
              Math.hypot(
                state.live.amplitude.mic,
                state.live.amplitude.speaker,
              ),
              1,
            )
          : 0,
        muted: isLive && state.live.muted,
        paused: isLive && state.live.paused,
        canPause: isLive && state.live.status === "active",
        setPaused: state.setPaused,
      };
    },
  );
  const bars = useLevelHistory(level);
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
  useHotkeys(PAUSE_RECORDING_HOTKEY, togglePaused, {
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
      className="cf-glass rounded-pill flex h-[46px] shrink-0 items-center gap-3 pr-[6px] pl-4 whitespace-nowrap [corner-shape:round]"
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
        <span
          aria-hidden="true"
          data-recording-level-meter
          className={cn([
            "flex h-[18px] items-center gap-[2px] motion-reduce:hidden @max-[560px]:hidden",
            muted && "opacity-40",
          ])}
        >
          {bars.map((value, index) => (
            <span
              key={index}
              className="w-[2px] rounded-[1px] bg-[var(--cf-label)]"
              style={{
                height: `${Math.max(3, Math.round(value * 18))}px`,
                opacity: 0.35 + value * 0.65,
              }}
            />
          ))}
        </span>
      )}
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
        className="cf-motion rounded-pill inline-flex h-7 shrink-0 items-center px-3 text-xs font-semibold text-white shadow-[inset_1px_1px_0_rgba(255,255,255,0.35),0_2px_8px_rgba(255,59,48,0.35)] transition-[filter] [corner-shape:round] hover:brightness-105 focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-default disabled:opacity-60"
        style={{
          background:
            "linear-gradient(180deg, rgba(255,255,255,0.3) 0%, rgba(255,255,255,0) 60%), var(--cf-record)",
        }}
      >
        {t`Stop`}
      </button>
    </div>
  );
}

/** Rolling window of recent input levels (newest on the right). */
function useLevelHistory(level: number) {
  const levelRef = useRef(level);
  levelRef.current = level;
  const [bars, setBars] = useState<number[]>(() =>
    Array.from({ length: METER_BARS }, () => 0),
  );

  useEffect(() => {
    const id = window.setInterval(() => {
      setBars((previous) => [...previous.slice(1), levelRef.current]);
    }, METER_SAMPLE_MS);
    return () => window.clearInterval(id);
  }, []);

  return bars;
}
