import { useLingui } from "@lingui/react/macro";
import { useEffect, useRef, useState } from "react";

import type {
  FloatingBarState,
  FloatingTranscriptBubble,
} from "@anlg/plugin-windows";
import {
  ArrowsInSimple,
  ArrowsOutSimple,
  CaretDown,
  WarningCircle,
} from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { DictationTranscript } from "./dictation";
import {
  FLOATING_BAR_COMPACT_GAP,
  FLOATING_BAR_COMPACT_HEIGHT,
  FLOATING_BAR_COMPACT_HORIZONTAL_PADDING,
  FLOATING_BAR_COMPACT_ICON_SIZE,
  FLOATING_BAR_COMPACT_SOLO_STOP_WIDTH,
  FLOATING_BAR_COMPACT_STOP_WIDTH,
  FLOATING_BAR_COMPACT_RADIUS,
  FLOATING_BAR_CONTROL_RADIUS,
  FLOATING_BAR_EXPANDED_RADIUS,
  FLOATING_BAR_HOVER_HANDLE_HEIGHT,
  FLOATING_BAR_HOVER_HANDLE_TOP_PADDING,
  FLOATING_BAR_HOVER_HANDLE_RESERVED_HEIGHT,
  FLOATING_BAR_INSET,
  compactControlsWidth,
} from "./layout";

import { formatElapsed } from "~/session/bookmark";
import { APP_NAME } from "~/shared/app-name";
import { LevelMeter } from "~/shared/level-meter";

/** Bars in the floating meter; fewer than the in-app pill to fit the bar. */
const FLOATING_METER_BARS = 10;

/**
 * Floating recording bar (non-macOS webview; macOS draws the same design
 * natively in FloatingBarView.swift). A neutral pill:
 * pulsing live dot · timer · level meter · Stop, plus the live-transcript
 * expand toggle. The window gets its theme through `state.colorScheme`, not
 * the app's `.dark` class, so colours mirror the `--cf-*` tokens here.
 */
export function FloatingBarOverlay({
  state,
  onStop,
  onToggleExpanded,
}: {
  state: FloatingBarState;
  onStop: () => void;
  onToggleExpanded: (expanded: boolean) => void;
}) {
  const [hovered, setHovered] = useState(false);
  const isExpanded =
    state.liveCaptionToggleVisible && !state.liveCaptionMinimized;
  const showsHoverHandle = hovered && !isExpanded;
  const colors = barColors(state);
  const controlsWidth = compactControlsWidth(state.liveCaptionToggleVisible);
  const expandsUpward = state.layout?.expandsUpward ?? false;

  return (
    <div
      className="relative h-full w-full font-sans"
      data-floating-bar-scheme={state.colorScheme}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {isExpanded && (
        <div
          data-tauri-drag-region
          className="absolute inset-x-0 top-0"
          style={{
            height:
              FLOATING_BAR_INSET + FLOATING_BAR_HOVER_HANDLE_RESERVED_HEIGHT,
          }}
        />
      )}
      <div
        data-floating-bar-surface
        className="absolute overflow-hidden"
        style={{
          left: FLOATING_BAR_INSET,
          right: FLOATING_BAR_INSET,
          bottom: FLOATING_BAR_INSET,
          top:
            FLOATING_BAR_INSET +
            (showsHoverHandle ? 0 : FLOATING_BAR_HOVER_HANDLE_RESERVED_HEIGHT),
          borderRadius: isExpanded
            ? FLOATING_BAR_EXPANDED_RADIUS
            : FLOATING_BAR_COMPACT_RADIUS,
          background:
            hovered && !isExpanded ? colors.envelopeSurface : colors.surface,
          // Hairline + a soft shadow that fits inside the window inset.
          boxShadow: `inset 0 0 0 0.5px ${colors.hairline}, 0 1px 3px ${colors.shadow}`,
        }}
      >
        {showsHoverHandle && <HoverHandle color={colors.handle} />}
        <div
          className="absolute inset-x-0 bottom-0"
          style={{
            top: showsHoverHandle
              ? FLOATING_BAR_HOVER_HANDLE_RESERVED_HEIGHT
              : 0,
          }}
        >
          {isExpanded && (
            <div
              className="absolute inset-x-0"
              style={{
                top: expandsUpward ? 0 : FLOATING_BAR_COMPACT_HEIGHT,
                bottom: expandsUpward ? FLOATING_BAR_COMPACT_HEIGHT : 0,
              }}
            >
              <TranscriptList
                key={state.dictation?.sessionId ?? "meeting"}
                dictation={state.dictation}
                bubbles={state.transcriptBubbles ?? []}
                colors={colors}
                notice={state.transcriptNotice ?? null}
              />
            </div>
          )}
          <div
            className="absolute flex -translate-x-1/2 items-center justify-center"
            style={{
              left: state.layout
                ? state.layout.controlsCenterX - FLOATING_BAR_INSET
                : `calc(100% - ${FLOATING_BAR_COMPACT_HORIZONTAL_PADDING + controlsWidth / 2}px)`,
              top: expandsUpward ? undefined : 0,
              bottom: expandsUpward ? 0 : undefined,
              width: controlsWidth,
              height: FLOATING_BAR_COMPACT_HEIGHT,
            }}
          >
            <FloatingControls
              state={state}
              isExpanded={isExpanded}
              colors={colors}
              onStop={onStop}
              onToggleExpanded={onToggleExpanded}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

function FloatingControls({
  state,
  isExpanded,
  colors,
  onStop,
  onToggleExpanded,
}: {
  state: FloatingBarState;
  isExpanded: boolean;
  colors: BarColors;
  onStop: () => void;
  onToggleExpanded: (expanded: boolean) => void;
}) {
  const { t } = useLingui();

  return (
    <div
      className="flex items-center"
      style={{ gap: FLOATING_BAR_COMPACT_GAP }}
    >
      <RecordingStatus state={state} colors={colors} onStop={onStop} />
      {state.liveCaptionToggleVisible ? (
        <button
          type="button"
          data-tauri-drag-region="false"
          aria-label={
            isExpanded ? t`Collapse live transcript` : t`Expand live transcript`
          }
          onClick={() => onToggleExpanded(!isExpanded)}
          className={cn([CONTROL_CLASS, "flex items-center justify-center"])}
          style={controlStyle(colors, {
            width: FLOATING_BAR_COMPACT_ICON_SIZE,
            height: FLOATING_BAR_COMPACT_ICON_SIZE,
            color: colors.secondary,
          })}
        >
          {isExpanded ? (
            <ArrowsInSimple size={14} weight="bold" aria-hidden="true" />
          ) : (
            <ArrowsOutSimple size={14} weight="bold" aria-hidden="true" />
          )}
        </button>
      ) : null}
    </div>
  );
}

/** Hover fill and keyboard-only focus ring, drawn from CSS variables. */
const CONTROL_CLASS =
  "transition-colors hover:bg-[var(--fb-hover)] focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--fb-accent)] disabled:cursor-default";

function controlStyle(colors: BarColors, style: React.CSSProperties) {
  return {
    borderRadius: FLOATING_BAR_CONTROL_RADIUS,
    "--fb-hover": colors.hover,
    "--fb-accent": colors.accent,
    ...style,
  } as React.CSSProperties;
}

/** Live dot · timer · level meter · Stop. */
function RecordingStatus({
  state,
  colors,
  onStop,
}: {
  state: FloatingBarState;
  colors: BarColors;
  onStop: () => void;
}) {
  const { t } = useLingui();
  const width = state.liveCaptionToggleVisible
    ? FLOATING_BAR_COMPACT_STOP_WIDTH
    : FLOATING_BAR_COMPACT_SOLO_STOP_WIDTH;
  const finishing = state.dictation?.phase === "transcribing";
  const notice = state.transcriptNotice ?? t`Recording needs attention`;
  const elapsed =
    !state.dictation && typeof state.elapsedSeconds === "number"
      ? formatElapsed(state.elapsedSeconds)
      : null;

  return (
    <div
      data-floating-bar-status
      className="flex items-center gap-2 pr-[3px] pl-2.5 whitespace-nowrap"
      style={{ width, height: FLOATING_BAR_COMPACT_ICON_SIZE }}
    >
      {state.status === "error" ? (
        <span
          role="img"
          aria-label={notice}
          title={t`${notice}. Open ${APP_NAME} to retry or change devices.`}
          className="flex shrink-0 items-center"
          style={{ color: colors.label }}
        >
          <WarningCircle size={14} weight="bold" aria-hidden="true" />
        </span>
      ) : (
        <span
          aria-hidden="true"
          data-floating-bar-live-dot
          className="cf-record-dot size-2 shrink-0 rounded-full"
          style={{ background: RECORD_RED }}
        />
      )}
      {elapsed ? (
        <span
          role="timer"
          aria-label={t`Recording time ${elapsed}`}
          className="shrink-0 text-[13px] font-semibold tabular-nums"
          style={{ color: colors.label }}
        >
          {elapsed}
        </span>
      ) : null}
      <span className="flex min-w-0 flex-1 items-center overflow-hidden">
        {finishing ? (
          <span
            role="status"
            className="truncate text-xs"
            style={{ color: colors.secondary }}
          >
            {t`Finishing…`}
          </span>
        ) : (
          <>
            <LevelMeter
              level={state.amplitude}
              bars={FLOATING_METER_BARS}
              height={16}
              color={colors.label}
              muted={state.status === "reconnecting"}
              data-floating-bar-meter
            />
            {state.status === "reconnecting" ? (
              <span role="status" className="sr-only">
                {t`Reconnecting…`}
              </span>
            ) : null}
          </>
        )}
      </span>
      <button
        type="button"
        data-tauri-drag-region="false"
        data-floating-bar-stop
        aria-label={
          state.dictation
            ? t`Finish dictation`
            : state.status === "error"
              ? t`${notice}; stop listening`
              : t`Stop listening`
        }
        title={state.dictation ? undefined : t`Stop listening`}
        disabled={finishing}
        onClick={onStop}
        className="inline-flex h-[24px] shrink-0 items-center rounded-full px-2.5 text-xs font-medium transition-opacity hover:opacity-85 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--fb-accent)] disabled:cursor-default disabled:opacity-60"
        style={
          {
            background: colors.label,
            color: colors.window,
            "--fb-accent": colors.accent,
          } as React.CSSProperties
        }
      >
        {state.dictation ? t`Done` : t`Stop`}
      </button>
    </div>
  );
}

function TranscriptList({
  bubbles,
  colors,
  dictation,
  notice,
}: {
  dictation: FloatingBarState["dictation"];
  bubbles: FloatingTranscriptBubble[];
  colors: BarColors;
  notice: string | null;
}) {
  const { t } = useLingui();
  const bottomRef = useRef<HTMLDivElement | null>(null);
  const [pinned, setPinned] = useState(true);

  useEffect(() => {
    if (pinned) {
      bottomRef.current?.scrollIntoView?.({ block: "end" });
    }
  }, [bubbles, pinned, notice, dictation?.text, dictation?.partial]);

  return (
    <div className="relative h-full p-3">
      <div
        className="h-full overflow-y-auto"
        onScroll={(event) => {
          const target = event.currentTarget;
          const distance =
            target.scrollHeight - target.scrollTop - target.clientHeight;
          setPinned(distance < 20);
        }}
      >
        <div
          className={cn([
            "flex min-h-full flex-col gap-2",
            dictation ? "justify-start" : "justify-end",
          ])}
        >
          {dictation ? (
            <DictationTranscript dictation={dictation} color={colors.label} />
          ) : (
            bubbles.map((bubble, index) => (
              <TranscriptBubble
                key={bubble.id}
                bubble={bubble}
                colors={colors}
                showsSpeakerLabel={
                  index === 0 ||
                  bubbles[index - 1]?.speakerLabel !== bubble.speakerLabel ||
                  bubbles[index - 1]?.isSelf !== bubble.isSelf
                }
              />
            ))
          )}
          {!dictation && notice ? (
            <p
              role="status"
              className="px-3 py-1.5 text-center text-[12px] leading-4"
              style={{ color: colors.secondary }}
            >
              {notice}
            </p>
          ) : null}
          <div ref={bottomRef} />
        </div>
      </div>
      {!pinned &&
      (dictation
        ? !!(dictation.text || dictation.partial)
        : bubbles.length > 0) ? (
        <button
          type="button"
          data-tauri-drag-region="false"
          onClick={() => {
            setPinned(true);
            bottomRef.current?.scrollIntoView?.({
              block: "end",
              behavior: "smooth",
            });
          }}
          className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-1.5 rounded-full px-3 py-1.5 text-[11px] font-medium focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--fb-accent)]"
          style={
            {
              background: colors.surfaceSolid,
              color: colors.label,
              boxShadow: `inset 0 0 0 0.5px ${colors.hairline}, 0 1px 3px ${colors.shadow}`,
              "--fb-accent": colors.accent,
            } as React.CSSProperties
          }
        >
          <CaretDown size={10} weight="bold" aria-hidden="true" />
          {t`Go to bottom`}
        </button>
      ) : null}
    </div>
  );
}

function TranscriptBubble({
  bubble,
  showsSpeakerLabel,
  colors,
}: {
  bubble: FloatingTranscriptBubble;
  showsSpeakerLabel: boolean;
  colors: BarColors;
}) {
  const overlapping = bubble.overlapsPrevious || bubble.overlapsNext;

  return (
    <div
      className={cn(["flex", bubble.isSelf ? "justify-end" : "justify-start"])}
    >
      <div className="max-w-[calc(100%-40px)] text-left">
        {(showsSpeakerLabel || overlapping) && (
          <p
            className="mb-1 text-[10px] font-semibold"
            style={{ color: colors.secondary }}
          >
            {showsSpeakerLabel ? bubble.speakerLabel : ""}
          </p>
        )}
        <p
          data-floating-transcript-bubble
          className="rounded-[11px] px-2.5 py-2 text-[13px] leading-5"
          style={{
            color: colors.label,
            background: bubble.isSelf ? colors.bubbleSelf : colors.bubble,
            boxShadow: overlapping
              ? `inset 0 0 0 1px ${colors.hairlineStrong}`
              : undefined,
          }}
        >
          {bubble.text}
        </p>
      </div>
    </div>
  );
}

function HoverHandle({ color }: { color: string }) {
  return (
    <div
      data-tauri-drag-region
      className="flex items-center justify-center"
      style={{
        height: FLOATING_BAR_HOVER_HANDLE_HEIGHT,
        width: "100%",
        paddingTop: FLOATING_BAR_HOVER_HANDLE_TOP_PADDING,
        boxSizing: "content-box",
      }}
    >
      <div
        data-tauri-drag-region
        className="h-full"
        style={{
          width: "calc(100% - 16px)",
          backgroundImage: `radial-gradient(circle, ${color} 0.8px, transparent 0.9px)`,
          backgroundSize: "5px 7px",
        }}
      />
    </div>
  );
}

/** `--cf-record`: the bar's only red. */
const RECORD_RED = "#ff3b30";

export type BarColors = {
  surface: string;
  surfaceSolid: string;
  envelopeSurface: string;
  hairline: string;
  hairlineStrong: string;
  shadow: string;
  label: string;
  secondary: string;
  window: string;
  hover: string;
  handle: string;
  bubble: string;
  bubbleSelf: string;
  accent: string;
};

/**
 * Neutral palette mirroring the `--cf-*` tokens (c-final.css). The surface
 * honours the floating bar opacity setting but never drops below legible.
 */
export function barColors(state: FloatingBarState): BarColors {
  const opacity = Math.min(Math.max(state.opacity, 0.35), 0.95);
  const alpha = 0.55 + ((opacity - 0.35) / 0.6) * 0.45;
  const dark = state.colorScheme === "dark";
  const surfaceRgb = dark ? "28, 28, 30" : "255, 255, 255";

  return {
    surface: `rgba(${surfaceRgb}, ${alpha.toFixed(3)})`,
    surfaceSolid: `rgb(${surfaceRgb})`,
    envelopeSurface: `rgb(${surfaceRgb})`,
    hairline: dark ? "rgba(255, 255, 255, 0.1)" : "rgba(0, 0, 0, 0.08)",
    hairlineStrong: dark ? "rgba(255, 255, 255, 0.24)" : "rgba(0, 0, 0, 0.2)",
    shadow: dark ? "rgba(0, 0, 0, 0.45)" : "rgba(0, 0, 0, 0.16)",
    label: dark ? "#f5f5f7" : "#1d1d1f",
    secondary: dark ? "#a1a1a6" : "#6e6e73",
    window: dark ? "#000000" : "#fbfbfd",
    hover: dark ? "rgba(255, 255, 255, 0.08)" : "rgba(0, 0, 0, 0.05)",
    handle: dark ? "rgba(255, 255, 255, 0.35)" : "rgba(0, 0, 0, 0.25)",
    bubble: dark ? "rgba(255, 255, 255, 0.08)" : "rgba(0, 0, 0, 0.05)",
    bubbleSelf: dark ? "rgba(255, 255, 255, 0.14)" : "rgba(0, 0, 0, 0.09)",
    accent: dark ? "#0a84ff" : "#0071e3",
  };
}
