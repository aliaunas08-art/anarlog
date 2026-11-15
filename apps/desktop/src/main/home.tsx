import { Trans, useLingui } from "@lingui/react/macro";
import { platform } from "@tauri-apps/plugin-os";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { commands as openerCommands } from "@anlg/plugin-opener2";
import {
  ClockCounterClockwise,
  LockSimple,
  Microphone,
  Plus,
  Sparkle,
} from "@anlg/ui/components/icons";
import { cn, parseEventInstant } from "@anlg/utils";

import { useAuth } from "~/auth";
import { useIgnoredEvents } from "~/calendar/ignored-events";
import { queueChatPrompt } from "~/chat/components/input/hooks";
import { usePersonalContact } from "~/contacts/queries";
import { useShell } from "~/contexts/shell";
import { useLiveQuery } from "~/db";
import { createSession, getOrCreateSessionForEventId } from "~/session/queries";
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

type UpcomingRow = {
  id: string;
  tracking_id_event: string;
  recurrence_series_id: string;
  title: string;
  started_at: string;
  meeting_link: string;
};

type RecentRow = { id: string; title: string; updated_at: string };

const UPCOMING_SQL = `
  SELECT e.id, e.tracking_id_event, e.recurrence_series_id, e.title,
    e.started_at, e.meeting_link
  FROM events e
  JOIN calendars c ON c.id = e.calendar_id
  WHERE c.enabled = 1
    AND c.deleted_at IS NULL
    AND e.deleted_at IS NULL
    AND e.is_all_day = 0
    AND julianday(e.started_at) > julianday(?)
    AND julianday(e.started_at) < julianday(?)
  ORDER BY julianday(e.started_at)
  LIMIT 10
`;

const RECENT_SQL = `
  SELECT id, title, updated_at
  FROM sessions
  WHERE deleted_at IS NULL
  ORDER BY updated_at DESC
  LIMIT 3
`;

const HOUR_MS = 60 * 60 * 1000;
const UPCOMING_WINDOW_MS = 12 * HOUR_MS;
// A meeting that started a few minutes ago can still be joined and recorded.
const STARTED_GRACE_MS = 10 * 60 * 1000;

function useMinuteNow() {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return Math.floor(now / 60_000) * 60_000;
}

function useFirstName(): string {
  const auth = useAuth();
  const localOwnerUserId = useOwnerUserId();
  const humanId = auth.session?.user.id ?? localOwnerUserId ?? "";
  const { data } = usePersonalContact(humanId);
  const metadataName = auth.session?.user.user_metadata?.full_name;
  const full =
    data?.name?.trim() ||
    (typeof metadataName === "string" ? metadataName.trim() : "");
  return full.split(/\s+/)[0] ?? "";
}

/** Keeps the last loaded value so re-keyed live queries don't flash empty. */
function useStable<T>(value: T | undefined): T | undefined {
  const last = useRef(value);
  if (value !== undefined) last.current = value;
  return last.current;
}

function useUpcomingMeeting(now: number) {
  const { isIgnored } = useIgnoredEvents();
  // Query an hour-aligned window so params (and the live query) change hourly,
  // then pick the next meeting on the client every minute.
  const hour = Math.floor(now / HOUR_MS) * HOUR_MS;
  const params = useMemo(
    () => [
      new Date(hour - STARTED_GRACE_MS).toISOString(),
      new Date(hour + HOUR_MS + UPCOMING_WINDOW_MS).toISOString(),
    ],
    [hour],
  );
  const { data } = useLiveQuery<UpcomingRow, UpcomingRow[]>({
    sql: UPCOMING_SQL,
    params,
    mapRows: (rows) => rows,
  });
  const rows = useStable(data) ?? [];
  return (
    rows.find((row) => {
      const start = parseEventInstant(row.started_at)?.getTime();
      return (
        start !== undefined &&
        start > now - STARTED_GRACE_MS &&
        start < now + UPCOMING_WINDOW_MS &&
        !isIgnored(row.tracking_id_event, row.recurrence_series_id)
      );
    }) ?? null
  );
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

export function HomeView() {
  const { t, i18n } = useLingui();
  const { chat } = useShell();
  const openCurrent = useTabs((state) => state.openCurrent);
  const mod = platform() === "macos" ? "⌘" : "Ctrl";
  const now = useMinuteNow();
  const firstName = useFirstName();
  const unexpectedQuit = useUnexpectedQuit();
  const upcoming = useUpcomingMeeting(now);
  const recents = useRecentNotes();
  const recovered = useRecoveredNote(unexpectedQuit, recents[0]);
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

  const newNote = useCallback(() => {
    const title = draft.trim();
    void createSession(title)
      .then((id) => {
        setDraft("");
        openSession(id);
      })
      .catch((error) => console.error("[home] failed to create note", error));
  }, [draft, openSession]);

  const recordInPerson = useCallback(() => {
    void createSession(draft.trim())
      .then((id) => {
        setDraft("");
        markSessionInPerson(id);
        openSessionAndListen(id, { behavior: "current" });
      })
      .catch((error) =>
        console.error("[home] failed to start in-person note", error),
      );
  }, [draft]);

  const ask = useCallback(
    (text: string) => {
      const prompt = text.trim();
      if (prompt) queueChatPrompt(chat.sessionId, prompt);
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

  const jumpBackIn = recents.filter((note) => note.id !== recovered?.id);

  return (
    <div
      data-tauri-drag-region
      className="flex h-full w-full flex-col items-center justify-center overflow-y-auto px-6 py-12 text-center"
    >
      <div className="flex w-full max-w-[680px] flex-col items-center gap-7">
        <header className="flex flex-col items-center gap-3">
          <h1 className="text-[44px] leading-[1.05] font-bold tracking-[-0.035em] whitespace-nowrap text-[var(--cf-label)]">
            {firstName ? `${greeting}, ${firstName}` : greeting}
          </h1>
          <p className="text-base tracking-[-0.01em] whitespace-nowrap text-[var(--cf-secondary)]">
            {subtitle}
          </p>
        </header>

        <form
          data-tauri-drag-region="false"
          onSubmit={(event) => {
            event.preventDefault();
            newNote();
          }}
          className="cf-hairline flex w-full flex-col gap-3.5 rounded-[20px] bg-[var(--cf-surface)] py-3 pr-3 pl-5 text-left shadow-[var(--cf-composer-shadow)]"
        >
          <input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                ask(draft);
                setDraft("");
              }
            }}
            aria-label={t`Note title or question`}
            placeholder={t`Start a note, or ask about any meeting…`}
            className="w-full bg-transparent pt-1 text-base text-[var(--cf-label)] outline-none placeholder:text-[var(--cf-tertiary)]"
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={recordInPerson}
              className="cf-hairline cf-motion inline-flex h-[30px] items-center gap-1.5 rounded-[999px] px-3 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
            >
              <Microphone className="size-3.5" />
              <Trans>In person</Trans>
            </button>
            <button
              type="button"
              onClick={() => {
                ask(draft);
                setDraft("");
              }}
              className="cf-hairline cf-motion inline-flex h-[30px] items-center gap-1.5 rounded-[999px] px-3 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
            >
              <Sparkle className="size-3.5" />
              <Trans>Ask</Trans>
              <span className="text-xs text-[var(--cf-tertiary)]">{mod}↩</span>
            </button>
            <button
              type="submit"
              className="cf-glass-tint cf-motion ml-auto inline-flex h-[34px] items-center gap-2 rounded-[999px] px-4 text-sm font-semibold whitespace-nowrap transition-opacity hover:opacity-90"
            >
              <Plus className="size-3.5" />
              <Trans>New note</Trans>
              <span className="text-xs font-medium opacity-75">↩</span>
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
              className="cf-hairline cf-motion h-8 rounded-[999px] bg-[var(--cf-chip)] px-3.5 text-[13px] whitespace-nowrap text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
            >
              {text}
            </button>
          ))}
        </div>

        {(recovered || upcoming) && (
          <div className="flex w-full flex-col gap-2 text-left">
            {recovered && (
              <HomeRow
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
            className="flex flex-col items-center gap-2.5"
          >
            <span className="text-xs font-semibold text-[var(--cf-secondary)]">
              <Trans>Jump back in</Trans>
            </span>
            <div className="flex flex-wrap justify-center gap-2">
              {jumpBackIn.map((note) => (
                <button
                  key={note.id}
                  type="button"
                  data-tauri-drag-region="false"
                  onClick={() => openSession(note.id)}
                  className="cf-hairline cf-motion inline-flex h-[30px] max-w-[260px] items-center gap-2 rounded-[999px] bg-[var(--cf-chip)] px-3 text-[13px] text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)]"
                >
                  <span className="truncate">
                    {note.title || t`Untitled note`}
                  </span>
                  <span className="shrink-0 text-[var(--cf-secondary)]">
                    {relativeDay(note.updated_at, now, i18n.locale, t)}
                  </span>
                </button>
              ))}
            </div>
          </nav>
        )}

        <p className="inline-flex items-center gap-1.5 text-xs whitespace-nowrap text-[var(--cf-secondary)]">
          <LockSimple className="size-3" />
          <Trans>Recordings and notes stay on this Mac</Trans>
        </p>
      </div>
    </div>
  );
}

function relativeDay(
  iso: string,
  now: number,
  locale: string,
  t: ReturnType<typeof useLingui>["t"],
) {
  const date = new Date(iso);
  const today = new Date(now);
  const startOf = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOf(today) - startOf(date)) / 86_400_000);
  if (days <= 0) return t`Today`;
  if (days === 1) return t`Yesterday`;
  if (days < 7) return date.toLocaleDateString(locale, { weekday: "short" });
  return date.toLocaleDateString(locale, { month: "short", day: "numeric" });
}

function HomeRow({
  icon,
  title,
  detail,
  action,
  onAction,
}: {
  icon: React.ReactNode;
  title: string;
  detail: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <section className="cf-hairline flex min-w-0 items-center gap-3 rounded-[14px] bg-[var(--cf-surface)] p-3.5">
      <div
        aria-hidden="true"
        className="flex size-9 shrink-0 items-center justify-center rounded-[10px] bg-[var(--cf-fill)] text-[var(--cf-label)]"
      >
        {icon}
      </div>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm font-semibold text-[var(--cf-label)]">
          {title}
        </span>
        <span className="truncate text-xs text-[var(--cf-secondary)]">
          {detail}
        </span>
      </div>
      <button
        type="button"
        data-tauri-drag-region="false"
        onClick={onAction}
        className={cn([
          "cf-motion h-[30px] shrink-0 rounded-[999px] px-3 text-[13px] font-medium whitespace-nowrap",
          "bg-[var(--cf-fill)] text-[var(--cf-label)] transition-opacity hover:opacity-80",
        ])}
      >
        {action}
      </button>
    </section>
  );
}
