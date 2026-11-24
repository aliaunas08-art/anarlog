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

// One flat capsule (subtle fill + hairline); everything inside is a 36px segment.
const BAR =
  "cf-seg rounded-pill pointer-events-auto relative flex h-[46px] max-w-full items-center gap-[2px] p-[5px] [corner-shape:round]";
const GLASS_ITEM =
  "cf-motion rounded-pill inline-flex h-9 shrink-0 items-center gap-[7px] px-4 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors select-none [corner-shape:round] hover:bg-[var(--cf-fill)] focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-none disabled:cursor-default disabled:opacity-60";

function BarDivider() {
  return (
    <span
      aria-hidden="true"
      data-session-bar-divider
      className="mx-1 h-5 w-px shrink-0 bg-[var(--cf-separator)]"
    />
  );
}

/**
 * Flat bar floating at the bottom of the session view, one capsule: view
 * segments (sliding solid thumb) | Record or the live recording controls ·
 * In person | Ask.
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
  // The recorder writes and fsyncs the audio file about once a second
  // (listener-core recorder/chunks.rs); with retention "none" it is deleted
  // after the meeting, so the pill only shows its "saved" mark when kept.
  const audioKept =
    normalizeAudioRetention(useConfigValue("audio_retention")) !== "none";

  return (
    // Full-width (inset-x) host, not left-1/2 + translate: an absolutely
    // positioned box at left:50% can only shrink-fit into the right half,
    // which squeezed the view tabs and clipped "Transcript".
    <div className="pointer-events-none absolute inset-x-4 bottom-[22px] z-30 flex flex-col-reverse items-center">
      <div
        role="toolbar"
        aria-label={t`Meeting controls`}
        data-session-bottom-bar
        // No glass shell around nothing (e.g. chat open with no actions).
        className={
          viewSwitcher || action || showTrailingGroup
            ? BAR
            : "pointer-events-auto relative flex"
        }
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
        {viewSwitcher && (action || showTrailingGroup) ? <BarDivider /> : null}
        {action?.kind === "stop" ? (
          <RecordingPill
            sessionId={tab.id}
            active={tab.active}
            disabled={action.disabled}
            stopTitle={action.title}
            onStop={action.onClick}
            onBookmark={onBookmark}
            audioSaved={audioKept}
          />
        ) : action ? (
          <PrimaryAction action={action} />
        ) : null}
        {showTrailingGroup ? (
          <div className="flex shrink-0 items-center gap-[2px]">
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
            {action?.showInPerson && showAsk ? <BarDivider /> : null}
            {showAsk ? (
              <button
                type="button"
                data-tauri-drag-region="false"
                aria-label={t`Ask Notes anything`}
                aria-keyshortcuts="Meta+P"
                title={t`Ask about this meeting (⌘P)`}
                onClick={() => chat.sendEvent({ type: "OPEN" })}
                className={GLASS_ITEM}
              >
                {t`Ask`}
                <kbd
                  aria-hidden="true"
                  data-session-ask-shortcut
                  className="font-sans text-xs text-[var(--cf-tertiary)] @max-[640px]:hidden"
                >
                  ⌘P
                </kbd>
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
              // A step smaller than the selected view tab, so the two solid
              // pills read as tab + action rather than two selections.
              "cf-motion rounded-pill mx-0.5 inline-flex h-8 max-w-56 shrink-0 items-center gap-2 px-3.5 text-[13px] font-medium whitespace-nowrap transition-opacity [corner-shape:round]",
              "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-1 focus-visible:outline-none",
              "disabled:cursor-default disabled:opacity-60",
              "bg-[var(--cf-label)] text-[var(--cf-window)] hover:opacity-85",
            ])}
          >
            {isRecord ? (
              <span
                aria-hidden="true"
                className="rounded-pill size-2 shrink-0 bg-current"
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
              {t`This is a prerecorded demo, so your camera stays off. Click Join & record to see Notes in action.`}
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
