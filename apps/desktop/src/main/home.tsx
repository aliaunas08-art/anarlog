import { Trans, useLingui } from "@lingui/react/macro";
import { platform } from "@tauri-apps/plugin-os";
import { useCallback, useEffect, useMemo, useState } from "react";

import { commands as openerCommands } from "@anlg/plugin-opener2";
import {
  CaretDown,
  ClockCounterClockwise,
  FileText,
  LockSimple,
  Microphone,
  Plus,
} from "@anlg/ui/components/icons";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@anlg/ui/components/ui/tooltip";
import { cn, parseEventInstant } from "@anlg/utils";

import { useAuth } from "~/auth";
import {
  type UpcomingMeetingRow,
  useMinuteNow,
  useUpcomingMeetings,
} from "~/calendar/upcoming-meetings";
import { queueChatPrompt } from "~/chat/components/input/hooks";
import { useChatContext } from "~/chat/state/chat-context";
import { usePersonalContact } from "~/contacts/queries";
import { useShell } from "~/contexts/shell";
import { useLiveQuery } from "~/db";
import {
  TemplatePickerPopover,
  type TemplateSelection,
} from "~/session/components/note-input/template-picker";
import {
  createSession,
  getOrCreateSessionForEventId,
  updateSession,
} from "~/session/queries";
import { AppMark } from "~/shared/app-mark";
import { useConfigValue } from "~/shared/config";
import { useOwnerUserId } from "~/shared/owner-user";
import {
  dismissUnexpectedQuit,
  useUnexpectedQuit,
} from "~/shared/unexpected-quit";
import { openSessionAndListen } from "~/shared/useNewNote";
import { useTabs } from "~/store/zustand/tabs";
import {
  hasAudioAwaitingUser,
  loadCaptureLifecycleMarker,
} from "~/stt/capture-lifecycle-storage";
import { markSessionInPerson } from "~/stt/useStartListening";
import { useUserTemplate } from "~/templates";
import { commands as desktopCommands } from "~/types/tauri.gen";

type RecentRow = { id: string; title: string; updated_at: string };

const RECENT_SQL = `
  SELECT id, title, updated_at
  FROM sessions
  WHERE deleted_at IS NULL
  ORDER BY updated_at DESC
  LIMIT 3
`;

let accountNamePromise: Promise<string> | null = null;

async function fetchAccountFullName(): Promise<string> {
  try {
    const name = await desktopCommands.getAccountFullName();
    return name?.trim() ?? "";
  } catch {
    return "";
  }
}

/** The macOS account's full name; a found name is cached for the app run. */
function loadAccountFullName(): Promise<string> {
  accountNamePromise ??= fetchAccountFullName().then((name) => {
    // An empty result is retried on the next mount.
    if (!name) accountNamePromise = null;
    return name;
  });
  return accountNamePromise;
}

function useAccountFullName(enabled: boolean): string {
  const [name, setName] = useState("");
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    void loadAccountFullName().then((value) => {
      if (active) setName(value);
    });
    return () => {
      active = false;
    };
  }, [enabled]);
  return enabled ? name : "";
}

function useFirstName(): string {
  const auth = useAuth();
  const localOwnerUserId = useOwnerUserId();
  const humanId = auth.session?.user.id ?? localOwnerUserId ?? "";
  const { data } = usePersonalContact(humanId);
  const metadataName = auth.session?.user.user_metadata?.full_name;
  const profileName =
    data?.name?.trim() ||
    (typeof metadataName === "string" ? metadataName.trim() : "");
  const accountName = useAccountFullName(!profileName);
  const full = profileName || accountName;
  return full.split(/\s+/)[0] ?? "";
}

function useRecentNotes() {
  const { data } = useLiveQuery<RecentRow, RecentRow[]>({
    sql: RECENT_SQL,
    mapRows: (rows) => rows,
  });
  return data ?? [];
}

/** After an unexpected quit, the most recent note if it has unsaved-to-transcript audio. */
function useRecoveredNote(unexpectedQuit: boolean | null, latest?: RecentRow) {
  const [recovered, setRecovered] = useState<RecentRow | null>(null);
  useEffect(() => {
    if (!unexpectedQuit || !latest) {
      setRecovered(null);
      return;
    }
    let active = true;
    void loadCaptureLifecycleMarker(latest.id)
      .then((marker) => {
        if (active && marker && hasAudioAwaitingUser(marker)) {
          setRecovered(latest);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [unexpectedQuit, latest]);
  return recovered;
}

const GREETING_SEED = Math.random();

/** Template the next note from Home is summarised with. `undefined` follows
 * the app default (Settings → selected template); `null` is Auto. */
function useHomeTemplate() {
  const defaultTemplateId = useConfigValue("selected_template_id") || null;
  const [picked, setPicked] = useState<TemplateSelection | undefined>();
  const templateId =
    picked === undefined ? defaultTemplateId : picked.templateId;
  const { data: template } = useUserTemplate(templateId);
  const title =
    picked?.title ?? (templateId ? template?.title || null : null) ?? null;
  return { templateId, title, setPicked };
}

export function HomeView() {
  const { t, i18n } = useLingui();
  const { chat } = useShell();
  const openCurrent = useTabs((state) => state.openCurrent);
  const mod = platform() === "macos" ? "⌘" : "Ctrl+";
  const now = useMinuteNow();
  const firstName = useFirstName();
  const unexpectedQuit = useUnexpectedQuit();
  const upcoming: UpcomingMeetingRow | null =
    useUpcomingMeetings(now)[0] ?? null;
  const recents = useRecentNotes();
  const recovered = useRecoveredNote(unexpectedQuit, recents[0]);
  const homeTemplate = useHomeTemplate();
  const [draft, setDraft] = useState("");

  const greeting = useMemo(() => {
    if (unexpectedQuit) return t`Welcome back`;
    const hour = new Date(now).getHours();
    const timeOfDay =
      hour < 12
        ? t`Good morning`
        : hour < 18
          ? t`Good afternoon`
          : t`Good evening`;
    const options = [t`Welcome back`, t`Jump back in`, timeOfDay];
    return options[Math.floor(GREETING_SEED * options.length)];
  }, [unexpectedQuit, now, t]);

  const subtitle = unexpectedQuit
    ? t`Everything from last time is safe. Ready when you are.`
    : t`Ready when you are.`;

  const openSession = useCallback(
    (id: string) => openCurrent({ type: "sessions", id }),
    [openCurrent],
  );

  const { templateId } = homeTemplate;
  /** Creates the note and pins the picked template for its summary. */
  const createHomeNote = useCallback(async () => {
    const id = await createSession(draft.trim());
    if (templateId) {
      await updateSession(id, { raw_md: "", raw_template_id: templateId });
    }
    setDraft("");
    return id;
  }, [draft, templateId]);

  const newNote = useCallback(() => {
    void createHomeNote()
      .then(openSession)
      .catch((error) => console.error("[home] failed to create note", error));
  }, [createHomeNote, openSession]);

  const recordInPerson = useCallback(() => {
    void createHomeNote()
      .then((id) => {
        markSessionInPerson(id);
        openSessionAndListen(id, { behavior: "current" });
      })
      .catch((error) =>
        console.error("[home] failed to start in-person note", error),
      );
  }, [createHomeNote]);

  const ask = useCallback(
    (text: string) => {
      const prompt = text.trim();
      if (prompt) {
        // Home has no open meeting: ask across all meetings.
        const threadSessionId = useChatContext
          .getState()
          .prepareAskThread("library", undefined);
        queueChatPrompt(threadSessionId, prompt);
      }
      chat.sendEvent({ type: "OPEN" });
    },
    [chat],
  );

  const recordUpcoming = useCallback(async () => {
    if (!upcoming) return;
    try {
      if (upcoming.meeting_link) {
        await openerCommands.openUrl(upcoming.meeting_link, null);
      }
      const id = await getOrCreateSessionForEventId(
        upcoming.id,
        upcoming.title || t`Untitled meeting`,
      );
      openSessionAndListen(id, { behavior: "current" });
    } catch (error) {
      console.error("[home] failed to start meeting note", error);
    }
  }, [upcoming, t]);

  const upcomingStart = upcoming
    ? parseEventInstant(upcoming.started_at)
    : null;
  const formatTime = (date: Date | null) =>
    (date ?? new Date(now)).toLocaleTimeString(i18n.locale, {
      hour: "numeric",
      minute: "2-digit",
    });

  const minutesUntil = upcomingStart
    ? Math.round((upcomingStart.getTime() - now) / 60_000)
    : 0;

  const suggestions = [
    t`What did I promise this week?`,
    t`Recap yesterday`,
    upcoming
      ? t`Prep me for ${upcoming.title || t`my next meeting`}`
      : t`Summarize my last meeting`,
  ];

  const jumpBackIn = recents
    .filter((note) => note.id !== recovered?.id)
    .slice(0, 2);
  const dayLabels = { today: t`Today`, yesterday: t`Yesterday` };

  return (
    <div data-tauri-drag-region className="h-full w-full overflow-y-auto">
      <div
        data-tauri-drag-region
        className="flex min-h-full flex-col items-center px-6 text-center"
      >
        <div
          data-tauri-drag-region
          className="flex w-full max-w-[660px] flex-1 flex-col items-center justify-center gap-7 pt-14 pb-10"
        >
          <header className="flex flex-col items-center gap-3">
            <AppTile />
            <h1 className="text-[40px] leading-[1.05] font-bold tracking-[-0.03em] whitespace-nowrap text-[var(--cf-label)]">
              {firstName ? `${greeting}, ${firstName}` : greeting}
            </h1>
            <p className="text-[15px] tracking-[-0.01em] whitespace-nowrap text-[var(--cf-secondary)]">
              {subtitle}
            </p>
          </header>

          <form
            data-tauri-drag-region="false"
            onSubmit={(event) => {
              // Return asks; only the New note button (or ⌘N) makes a note.
              event.preventDefault();
              if (!draft.trim()) return;
              ask(draft);
              setDraft("");
            }}
            className="cf-hairline flex w-full flex-col gap-3.5 rounded-[16px] bg-[var(--cf-surface)] pt-4 pr-3 pb-3 pl-5 text-left shadow-[var(--cf-composer-shadow)] [corner-shape:round]"
          >
            <input
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => {
                // ⌘N from the field titles the new note with the typed text.
                if (
                  event.key.toLowerCase() === "n" &&
                  (event.metaKey || event.ctrlKey) &&
                  !event.shiftKey &&
                  !event.altKey
                ) {
                  event.preventDefault();
                  event.stopPropagation();
                  newNote();
                }
              }}
              aria-label={t`Question or note title`}
              placeholder={t`Ask about your meetings, or start a note…`}
              className="w-full bg-transparent text-base text-[var(--cf-label)] outline-none placeholder:text-[var(--cf-tertiary)]"
            />
            <div className="flex min-w-0 items-center gap-2">
              <TemplatePickerPopover
                usedTemplateId={homeTemplate.templateId}
                onSelectTemplate={homeTemplate.setPicked}
                trigger={
                  <button
                    type="button"
                    aria-label={t`Template: ${homeTemplate.title ?? t`Auto`}`}
                    className="cf-hairline cf-motion rounded-pill inline-flex h-[30px] min-w-0 items-center gap-1.5 px-3 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
                  >
                    <FileText className="size-[13px] shrink-0" />
                    <span className="max-w-[160px] truncate">
                      {homeTemplate.title ?? t`Auto`}
                    </span>
                    <CaretDown
                      weight="bold"
                      className="size-2.5 shrink-0 text-[var(--cf-secondary)]"
                    />
                  </button>
                }
              />
              <TooltipProvider>
                <Tooltip delayDuration={300}>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      onClick={recordInPerson}
                      aria-label={t`In person`}
                      className="cf-hairline cf-motion rounded-pill inline-flex size-[30px] shrink-0 items-center justify-center text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
                    >
                      <Microphone className="size-3.5" />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    <Trans>In person · record with the mic only</Trans>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
              <button
                type="button"
                onClick={newNote}
                title={`${mod}N`}
                aria-keyshortcuts={mod === "⌘" ? "Meta+N" : "Control+N"}
                className="cf-motion rounded-pill ml-auto inline-flex h-7 shrink-0 items-center gap-1 bg-[var(--cf-label)] pr-3 pl-2.5 text-[13px] font-medium whitespace-nowrap text-[var(--cf-window)] transition-opacity hover:opacity-85 focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--cf-surface)] focus-visible:outline-hidden"
              >
                <Plus weight="bold" className="size-3" />
                <Trans>New note</Trans>
              </button>
            </div>
          </form>

          <div
            role="group"
            aria-label={t`Suggestions`}
            className="flex flex-wrap justify-center gap-2"
          >
            {suggestions.map((text) => (
              <button
                key={text}
                type="button"
                data-tauri-drag-region="false"
                onClick={() => ask(text)}
                className="cf-hairline cf-motion rounded-pill h-8 bg-[var(--cf-chip)] px-3.5 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
              >
                {text}
              </button>
            ))}
          </div>

          {(recovered || upcoming) && (
            <div className="flex w-full flex-col gap-2 text-left">
              {recovered && (
                <HomeRow
                  label={t`Recovered recording`}
                  icon={<ClockCounterClockwise className="size-[18px]" />}
                  title={t`${recovered.title || t`Your last note`} was recovered`}
                  detail={t`Audio was saved up to the second · not transcribed yet`}
                  action={t`Open`}
                  onAction={() => {
                    dismissUnexpectedQuit();
                    openSession(recovered.id);
                  }}
                />
              )}
              {upcoming && (
                <HomeRow
                  label={t`Up next`}
                  icon={
                    <span className="flex flex-col items-center leading-none">
                      <span className="text-[8px] font-bold text-[var(--cf-record)] uppercase">
                        {(upcomingStart ?? new Date(now)).toLocaleDateString(
                          i18n.locale,
                          { month: "short" },
                        )}
                      </span>
                      <span className="text-[15px] font-bold">
                        {(upcomingStart ?? new Date(now)).getDate()}
                      </span>
                    </span>
                  }
                  title={t`${upcoming.title || t`Untitled meeting`} at ${formatTime(upcomingStart)}`}
                  detail={
                    minutesUntil === 0
                      ? t`Starting now`
                      : minutesUntil < 0
                        ? t`Started ${-minutesUntil} min ago`
                        : minutesUntil < 60
                          ? t`Starts in ${minutesUntil} min`
                          : t`Up next from your calendar`
                  }
                  action={upcoming.meeting_link ? t`Join & record` : t`Record`}
                  onAction={() => void recordUpcoming()}
                />
              )}
            </div>
          )}

          {jumpBackIn.length > 0 && (
            <nav
              aria-label={t`Jump back in`}
              className="flex w-full flex-col items-center gap-2.5"
            >
              <span className="text-xs font-semibold text-[var(--cf-secondary)]">
                <Trans>Jump back in</Trans>
              </span>
              <div className="flex max-w-full flex-wrap justify-center gap-2">
                {jumpBackIn.map((note) => (
                  <button
                    key={note.id}
                    type="button"
                    data-tauri-drag-region="false"
                    onClick={() => openSession(note.id)}
                    className="cf-hairline cf-motion rounded-pill inline-flex h-[30px] max-w-[260px] min-w-0 items-center gap-2 bg-[var(--cf-chip)] px-3 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
                  >
                    <span className="truncate">
                      {note.title || t`Untitled note`}
                    </span>
                    <span className="shrink-0 text-[var(--cf-secondary)]">
                      {relativeDay(
                        note.updated_at,
                        now,
                        i18n.locale,
                        dayLabels,
                      )}
                    </span>
                  </button>
                ))}
              </div>
            </nav>
          )}
        </div>

        <p className="inline-flex shrink-0 items-center gap-1.5 pb-5 text-xs whitespace-nowrap text-[var(--cf-secondary)]">
          <LockSimple className="size-[11px]" />
          <Trans>Recordings and notes stay on this Mac</Trans>
        </p>
      </div>
    </div>
  );
}

/** The Timbre mark, in the label colour. */
function AppTile() {
  return <AppMark className="w-[68px] text-[var(--cf-label)]" />;
}

function relativeDay(
  iso: string,
  now: number,
  locale: string,
  labels: { today: string; yesterday: string },
) {
  const date = new Date(iso);
  const today = new Date(now);
  const startOf = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOf(today) - startOf(date)) / 86_400_000);
  if (days <= 0) return labels.today;
  if (days === 1) return labels.yesterday;
  if (days < 7) return date.toLocaleDateString(locale, { weekday: "short" });
  return date.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

function HomeRow({
  label,
  icon,
  title,
  detail,
  action,
  onAction,
}: {
  label: string;
  icon: React.ReactNode;
  title: string;
  detail: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <section
      aria-label={label}
      className="cf-hairline flex min-w-0 items-center gap-3 rounded-[14px] bg-[var(--cf-surface)] p-3.5"
    >
      <div
        aria-hidden="true"
        className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-[var(--cf-fill)] text-[var(--cf-label)]"
      >
        {icon}
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm leading-5 font-semibold text-[var(--cf-label)]">
          {title}
        </span>
        <span className="truncate text-xs leading-4 text-[var(--cf-secondary)]">
          {detail}
        </span>
      </div>
      <button
        type="button"
        data-tauri-drag-region="false"
        onClick={onAction}
        className={cn([
          "cf-motion rounded-pill h-[30px] shrink-0 px-3 text-[13px] font-medium whitespace-nowrap",
          "bg-[var(--cf-fill)] text-[var(--cf-label)] transition-opacity hover:opacity-80",
        ])}
      >
        {action}
      </button>
    </section>
  );
}
