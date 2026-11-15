import { useLingui } from "@lingui/react/macro";

import { Microphone } from "@anlg/ui/components/icons";
import {
  Popover,
  PopoverAnchor,
  PopoverContent,
} from "@anlg/ui/components/ui/popover";
import { cn } from "@anlg/utils";

import {
  type SessionMeetingAction,
  useSessionMeetingAction,
} from "./meeting-action";
import { RecordingPill } from "./recording-pill";
import { setSessionFabSelectionHost } from "./selection-slot";

import { useShell } from "~/contexts/shell";
import { normalizeAudioRetention } from "~/services/audio-retention-policy";
import { useConfigValue } from "~/shared/config";
import type { Tab } from "~/store/zustand/tabs/schema";

const GLASS_GROUP =
  "cf-glass rounded-pill flex h-[46px] shrink-0 items-center gap-[2px] p-[5px] [corner-shape:round]";
const GLASS_ITEM =
  "cf-motion rounded-pill inline-flex h-9 shrink-0 items-center gap-[7px] px-4 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors select-none [corner-shape:round] hover:bg-[var(--cf-glass-sel)] focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none disabled:cursor-default disabled:opacity-60";

/**
 * Liquid Glass bar floating at the bottom of the session view:
 * view tabs · primary recording action · In person / Ask.
 */
export function SessionBottomBar({
  tab,
  viewSwitcher,
  onBookmark,
}: {
  tab: Extract<Tab, { type: "sessions" }>;
  viewSwitcher?: React.ReactNode;
  onBookmark?: (elapsedSeconds: number) => void;
}) {
  const { t } = useLingui();
  const { chat } = useShell();
  const action = useSessionMeetingAction(tab.id);
  // Ask is the chat's entry point, so it leaves while chat is open; the rest
  // of the bar (tabs, recording pill) stays reachable.
  const showAsk = chat.mode === "FloatingClosed";
  const showTrailingGroup = showAsk || Boolean(action?.showInPerson);
  const isRecording = action?.kind === "stop";
  // The recorder writes and fsyncs the audio file about once a second
  // (listener-core recorder/chunks.rs); with retention "none" it is deleted
  // after the meeting, so only claim it is saved when it is kept.
  const audioKept =
    normalizeAudioRetention(useConfigValue("audio_retention")) !== "none";

  return (
    <div className="pointer-events-none absolute bottom-[22px] left-1/2 z-30 flex max-w-[calc(100%-2rem)] -translate-x-1/2 flex-col-reverse items-center">
      <div
        role="toolbar"
        aria-label={t`Meeting controls`}
        data-session-bottom-bar
        className="pointer-events-auto relative flex max-w-full items-center gap-2.5"
      >
        <span
          aria-hidden="true"
          // The floating chat panel rests its bottom edge on this marker, so it
          // opens just above the bar instead of covering it.
          data-chat-cta-trigger
          className="pointer-events-none absolute bottom-full left-0 mb-2 h-0 w-full"
        />
        {viewSwitcher ? (
          <div data-session-bottom-bar-views className="min-w-0 shrink">
            {viewSwitcher}
          </div>
        ) : null}
        {isRecording && audioKept ? (
          <p
            data-session-audio-saved
            className="pointer-events-none absolute top-full left-1/2 m-0 mt-1 -translate-x-1/2 text-[11px] whitespace-nowrap text-[var(--cf-tertiary)]"
          >
            {t`Audio saved to this Mac`}
          </p>
        ) : null}
        {action?.kind === "stop" ? (
          <RecordingPill
            sessionId={tab.id}
            active={tab.active}
            disabled={action.disabled}
            stopTitle={action.title}
            onStop={action.onClick}
            onBookmark={onBookmark}
          />
        ) : action ? (
          <PrimaryAction action={action} />
        ) : null}
        {showTrailingGroup ? (
          <div className={GLASS_GROUP}>
            {action?.showInPerson ? (
              <button
                type="button"
                data-tauri-drag-region="false"
                aria-label={t`Record in person`}
                title={t`Record in person (microphone only)`}
                disabled={action.disabled}
                onClick={action.startInPerson}
                className={cn([GLASS_ITEM, "@max-[640px]:px-2.5"])}
              >
                <Microphone aria-hidden className="size-3.5 shrink-0" />
                <span className="@max-[640px]:sr-only">{t`In person`}</span>
              </button>
            ) : null}
            {action?.showInPerson && showAsk ? (
              <span
                aria-hidden="true"
                className="h-5 w-px shrink-0 bg-[var(--cf-separator)]"
              />
            ) : null}
            {showAsk ? (
              <button
                type="button"
                data-tauri-drag-region="false"
                aria-label={t`Ask Anarlog anything`}
                title={t`Ask about this meeting`}
                onClick={() => chat.sendEvent({ type: "OPEN" })}
                className={GLASS_ITEM}
              >
                {t`Ask`}
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
      <div
        ref={setSessionFabSelectionHost}
        data-session-fab-selection
        className="pointer-events-auto z-10 mb-2"
      />
    </div>
  );
}

function PrimaryAction({ action }: { action: SessionMeetingAction }) {
  const { t } = useLingui();
  const isRecord = action.kind === "record";

  return (
    <Popover open={action.showWelcomeDemoPrompt}>
      <div className="relative flex shrink-0 items-center">
        <PopoverAnchor asChild>
          <button
            type="button"
            data-tauri-drag-region="false"
            data-session-primary-action={action.kind}
            aria-label={action.label}
            title={action.title}
            disabled={action.disabled}
            onClick={action.onClick}
            className={cn([
              "cf-motion rounded-pill inline-flex h-[46px] max-w-56 shrink-0 items-center gap-2 px-5 text-[13px] font-semibold whitespace-nowrap transition-[opacity,filter] [corner-shape:round]",
              "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-2 focus-visible:outline-none",
              "disabled:cursor-default disabled:opacity-60",
              "cf-glass-tint hover:brightness-105",
            ])}
          >
            {isRecord ? (
              <span
                aria-hidden="true"
                className="rounded-pill size-2 shrink-0 bg-[var(--cf-record)] shadow-[0_0_0_2px_rgba(255,255,255,0.6)]"
              />
            ) : (
              action.icon
            )}
            <span className="truncate">{action.label}</span>
          </button>
        </PopoverAnchor>
        {action.showWelcomeDemoPrompt ? (
          <PopoverContent
            data-welcome-demo-prompt
            side="top"
            sideOffset={10}
            onOpenAutoFocus={(event) => event.preventDefault()}
            className="border-border bg-popover text-popover-foreground pointer-events-none w-72 max-w-[calc(100vw-1rem)] rounded-md border px-3 py-2.5 text-sm shadow-sm"
          >
            <span
              data-welcome-demo-prompt-tail
              aria-hidden="true"
              className="border-border bg-popover absolute -bottom-1.5 left-1/2 size-3 -translate-x-1/2 rotate-45 border-r border-b"
            />
            <span className="relative block font-medium">{t`Try the demo`}</span>
            <span className="text-muted-foreground relative mt-0.5 block leading-snug">
              {t`This is a prerecorded demo, so your camera stays off. Click Join & record to see Anarlog in action.`}
            </span>
          </PopoverContent>
        ) : action.countdownLabel ? (
          <div
            data-header-meeting-countdown
            className="border-border bg-popover text-popover-foreground pointer-events-none absolute bottom-full left-1/2 z-20 mb-2 -translate-x-1/2 rounded-md border px-2.5 py-1 font-mono text-xs whitespace-nowrap tabular-nums shadow-sm"
          >
            <span
              data-header-meeting-countdown-tail
              aria-hidden="true"
              className="border-border bg-popover absolute -bottom-1.5 left-1/2 size-3 -translate-x-1/2 rotate-45 border-r border-b"
            />
            <span className="relative">{action.countdownLabel}</span>
          </div>
        ) : null}
      </div>
    </Popover>
  );
}
