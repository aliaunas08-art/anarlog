import { Trans, useLingui } from "@lingui/react/macro";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { CaretLeft, CaretRight } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { useSidebarNotes } from "./note-filter";
import {
  countTimelineSessionsByDay,
  filterTimelineTablesByFolder,
} from "./timeline/utils";

import {
  CALENDAR_DAY_MARKER_CLASS,
  CALENDAR_TODAY_MARKER_CLASS,
} from "~/calendar/components/day-cell";
import { useWeekStartsOn } from "~/calendar/hooks";
import { useTimelineSessionsTable } from "~/calendar/queries";
import { useConfigValue } from "~/shared/config";
import { useCurrentDay } from "~/shared/hooks/useCurrentDay";

type MonthView = { year: number; month: number };

const DAY_MS = 24 * 60 * 60 * 1000;

function pad(value: number) {
  return String(value).padStart(2, "0");
}

function toDayKey(year: number, month: number, day: number) {
  return `${year}-${pad(month + 1)}-${pad(day)}`;
}

function parseDayKey(dayKey: string) {
  const [year, month, day] = dayKey.split("-").map(Number);
  return { year, month: month - 1, day };
}

/** Noon UTC of the day: formatting with timeZone "UTC" never shifts the day. */
function dayKeyToUtcDate(dayKey: string) {
  const { year, month, day } = parseDayKey(dayKey);
  return new Date(Date.UTC(year, month, day, 12));
}

function addDays(dayKey: string, days: number) {
  const date = new Date(dayKeyToUtcDate(dayKey).getTime() + days * DAY_MS);
  return toDayKey(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate());
}

function shiftMonth({ year, month }: MonthView, delta: number): MonthView {
  const index = year * 12 + month + delta;
  return { year: Math.floor(index / 12), month: ((index % 12) + 12) % 12 };
}

function daysInMonth({ year, month }: MonthView) {
  return new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
}

function clampDayToMonth(view: MonthView, day: number) {
  return toDayKey(view.year, view.month, Math.min(day, daysInMonth(view)));
}

function monthOf(dayKey: string): MonthView {
  const { year, month } = parseDayKey(dayKey);
  return { year, month };
}

function isSameMonth(dayKey: string, view: MonthView) {
  const { year, month } = parseDayKey(dayKey);
  return year === view.year && month === view.month;
}

/** Always six rows of 7, padded with the adjacent months' days (as in
 * Google Calendar's mini month), so the grid height never jumps. */
function buildMonthWeeks(view: MonthView, weekStartsOn: 0 | 1): string[][] {
  const firstWeekday = new Date(Date.UTC(view.year, view.month, 1)).getUTCDay();
  const leading = (firstWeekday - weekStartsOn + 7) % 7;
  const start = addDays(toDayKey(view.year, view.month, 1), -leading);

  const weeks: string[][] = [];
  for (let week = 0; week < 6; week++) {
    weeks.push(
      Array.from({ length: 7 }, (_, day) => addDays(start, week * 7 + day)),
    );
  }
  return weeks;
}

export function SidebarDateFilter({
  folderFilter = null,
  className,
}: {
  folderFilter?: string | null;
  className?: string;
}) {
  const { t, i18n } = useLingui();
  const timezone = useConfigValue("timezone") || undefined;
  const weekStartsOn = useWeekStartsOn();
  const todayKey = useCurrentDay(timezone);
  const dateFilter = useSidebarNotes((state) => state.dateFilter);
  const setDateFilter = useSidebarNotes((state) => state.setDateFilter);
  const timelineSessionsTable = useTimelineSessionsTable();

  const [view, setView] = useState<MonthView>(() =>
    monthOf(dateFilter ?? todayKey),
  );
  const [focusKey, setFocusKey] = useState<string>(
    () => dateFilter ?? todayKey,
  );
  const shouldMoveFocusRef = useRef(false);
  const gridRef = useRef<HTMLDivElement | null>(null);

  // Keep the picked day visible when it changes from elsewhere.
  useEffect(() => {
    if (dateFilter === null) {
      return;
    }
    setFocusKey(dateFilter);
    setView((current) =>
      isSameMonth(dateFilter, current) ? current : monthOf(dateFilter),
    );
  }, [dateFilter]);

  // Notes per day come from the same live sessions table, folder scope and
  // timestamp rule as the timeline, so dots and counts match the list exactly
  // and nothing reloads (or flickers) when the month changes.
  const countsByDay = useMemo(() => {
    const { timelineSessionsTable: scoped } = filterTimelineTablesByFolder({
      folderFilter,
      timelineEventsTable: {},
      timelineSessionsTable,
    });
    return countTimelineSessionsByDay(scoped, timezone);
  }, [folderFilter, timelineSessionsTable, timezone]);

  const weeks = useMemo(
    () => buildMonthWeeks(view, weekStartsOn),
    [view, weekStartsOn],
  );

  const formatters = useMemo(
    () => ({
      month: new Intl.DateTimeFormat(i18n.locale, {
        month: "long",
        year: "numeric",
        timeZone: "UTC",
      }),
      weekday: new Intl.DateTimeFormat(i18n.locale, {
        weekday: "narrow",
        timeZone: "UTC",
      }),
      weekdayLong: new Intl.DateTimeFormat(i18n.locale, {
        weekday: "long",
        timeZone: "UTC",
      }),
      day: new Intl.DateTimeFormat(i18n.locale, {
        day: "numeric",
        timeZone: "UTC",
      }),
      fullDate: new Intl.DateTimeFormat(i18n.locale, {
        weekday: "long",
        month: "long",
        day: "numeric",
        year: "numeric",
        timeZone: "UTC",
      }),
      shortDate: new Intl.DateTimeFormat(i18n.locale, {
        weekday: "short",
        month: "short",
        day: "numeric",
        timeZone: "UTC",
      }),
    }),
    [i18n.locale],
  );

  const monthDate = new Date(Date.UTC(view.year, view.month, 1, 12));
  const monthLabel = formatters.month.format(monthDate);
  // 2023-01-01 was a Sunday; offset by the configured week start.
  const weekdayKeys = Array.from({ length: 7 }, (_, index) =>
    toDayKey(2023, 0, 1 + ((index + weekStartsOn) % 7)),
  );

  const notesLabel = (count: number) =>
    count === 0 ? t`No notes` : count === 1 ? t`1 note` : t`${count} notes`;

  // Roving tabindex: the focused day if it is in view, else the picked day,
  // today, or the 1st of the month.
  const tabStopKey = isSameMonth(focusKey, view)
    ? focusKey
    : dateFilter && isSameMonth(dateFilter, view)
      ? dateFilter
      : isSameMonth(todayKey, view)
        ? todayKey
        : toDayKey(view.year, view.month, 1);

  useEffect(() => {
    if (!shouldMoveFocusRef.current) {
      return;
    }
    shouldMoveFocusRef.current = false;
    gridRef.current
      ?.querySelector<HTMLElement>(`[data-day="${focusKey}"]`)
      ?.focus();
  }, [focusKey, view]);

  const changeMonth = (delta: number) => {
    const next = shiftMonth(view, delta);
    setView(next);
    setFocusKey(clampDayToMonth(next, parseDayKey(tabStopKey).day));
  };

  const toggleDay = (dayKey: string) => {
    setFocusKey(dayKey);
    if (!isSameMonth(dayKey, view)) {
      setView(monthOf(dayKey));
    }
    setDateFilter(dateFilter === dayKey ? null : dayKey);
  };

  const moveFocus = (dayKey: string) => {
    shouldMoveFocusRef.current = true;
    setFocusKey(dayKey);
    if (!isSameMonth(dayKey, view)) {
      setView(monthOf(dayKey));
    }
  };

  const handleGridKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const current = tabStopKey;
    const weekday =
      (dayKeyToUtcDate(current).getUTCDay() - weekStartsOn + 7) % 7;
    let next: string | null = null;

    switch (event.key) {
      case "ArrowLeft":
        next = addDays(current, -1);
        break;
      case "ArrowRight":
        next = addDays(current, 1);
        break;
      case "ArrowUp":
        next = addDays(current, -7);
        break;
      case "ArrowDown":
        next = addDays(current, 7);
        break;
      case "Home":
        next = addDays(current, -weekday);
        break;
      case "End":
        next = addDays(current, 6 - weekday);
        break;
      case "PageUp":
        next = clampDayToMonth(shiftMonth(view, -1), parseDayKey(current).day);
        break;
      case "PageDown":
        next = clampDayToMonth(shiftMonth(view, 1), parseDayKey(current).day);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        toggleDay(current);
        return;
      case "Escape":
        if (dateFilter !== null) {
          event.preventDefault();
          event.stopPropagation();
          setDateFilter(null);
        }
        return;
      default:
        return;
    }

    event.preventDefault();
    moveFocus(next);
  };

  const selectedCount = dateFilter ? (countsByDay.get(dateFilter) ?? 0) : 0;

  const navButtonClassName =
    "cf-motion inline-flex size-8 items-center justify-center rounded-pill text-[var(--cf-label)] transition-colors hover:bg-[var(--cf-fill)] focus-visible:bg-[var(--cf-fill)] focus-visible:outline-hidden";

  return (
    <section
      aria-label={t`By date`}
      data-sidebar-date-filter
      className={cn(["flex shrink-0 flex-col gap-1 px-2 pb-2", className])}
    >
      <div className="flex items-center justify-between gap-2 pr-0.5 pl-2.5">
        <span
          data-testid="date-filter-month"
          className="min-w-0 truncate text-[17px] leading-8 font-normal text-[var(--cf-label)]"
        >
          {monthLabel}
        </span>
        <span className="inline-flex shrink-0 items-center gap-1">
          <button
            type="button"
            aria-label={t`Previous month`}
            title={t`Previous month`}
            onClick={() => changeMonth(-1)}
            className={navButtonClassName}
          >
            <CaretLeft size={16} aria-hidden="true" />
          </button>
          <button
            type="button"
            aria-label={t`Next month`}
            title={t`Next month`}
            onClick={() => changeMonth(1)}
            className={navButtonClassName}
          >
            <CaretRight size={16} aria-hidden="true" />
          </button>
        </span>
      </div>

      <div
        ref={gridRef}
        role="grid"
        aria-label={monthLabel}
        onKeyDown={handleGridKeyDown}
        className="flex flex-col"
      >
        <div role="row" className="grid grid-cols-7 text-center">
          {weekdayKeys.map((key) => {
            const date = dayKeyToUtcDate(key);
            return (
              <span
                key={key}
                role="columnheader"
                aria-label={formatters.weekdayLong.format(date)}
                className="text-[12px] leading-8 font-medium text-[var(--cf-secondary)]"
              >
                {formatters.weekday.format(date)}
              </span>
            );
          })}
        </div>
        {weeks.map((week) => (
          <div key={week[0]} role="row" className="grid grid-cols-7">
            {week.map((dayKey) => {
              const isOutside = !isSameMonth(dayKey, view);
              const count = countsByDay.get(dayKey) ?? 0;
              const isSelected = dateFilter === dayKey;
              const isToday = dayKey === todayKey;
              const date = dayKeyToUtcDate(dayKey);
              const label = [
                formatters.fullDate.format(date),
                isToday ? t`Today` : null,
                notesLabel(count),
              ]
                .filter(Boolean)
                .join(", ");

              return (
                <span
                  key={dayKey}
                  role="gridcell"
                  aria-selected={isSelected}
                  aria-current={isToday ? "date" : undefined}
                  aria-label={label}
                  data-day={dayKey}
                  data-outside={isOutside || undefined}
                  data-has-notes={count > 0 || undefined}
                  tabIndex={dayKey === tabStopKey ? 0 : -1}
                  onClick={() => toggleDay(dayKey)}
                  onFocus={() => setFocusKey(dayKey)}
                  className="group/day relative flex h-9 cursor-default items-center justify-center outline-hidden select-none"
                >
                  {/* Same marker as the Calendar tab's month grid: the number
                   * sits alone in a centred square; the notes dot hangs below
                   * it so it never nudges the number. */}
                  <span
                    className={cn([
                      CALENDAR_DAY_MARKER_CLASS,
                      "cf-motion tabular-nums transition-colors",
                      "group-focus-visible/day:ring-2 group-focus-visible/day:ring-[var(--cf-accent)]",
                      isToday
                        ? [
                            CALENDAR_TODAY_MARKER_CLASS,
                            isSelected &&
                              "ring-2 ring-[var(--cf-accent)] ring-offset-1 ring-offset-[var(--cf-sidebar-bg)]",
                          ]
                        : isSelected
                          ? "bg-[var(--cf-accent)] font-medium text-[var(--cf-on-accent)]"
                          : [
                              "group-hover/day:bg-[var(--cf-fill)]",
                              isOutside
                                ? "text-[var(--cf-tertiary)]"
                                : "text-[var(--cf-label)]",
                            ],
                    ])}
                  >
                    {formatters.day.format(date)}
                  </span>
                  {count > 0 && !isOutside ? (
                    <span
                      aria-hidden="true"
                      data-testid="date-filter-dot"
                      className={cn([
                        "rounded-pill absolute bottom-0 left-1/2 size-[3px] -translate-x-1/2",
                        isSelected && !isToday
                          ? "bg-[var(--cf-accent)]"
                          : "bg-[var(--cf-secondary)]",
                      ])}
                    />
                  ) : null}
                </span>
              );
            })}
          </div>
        ))}
      </div>

      {dateFilter ? (
        <div className="flex items-center justify-between gap-2 px-2.5 pt-1 text-[12px] text-[var(--cf-secondary)]">
          <span className="min-w-0 truncate" aria-live="polite">
            {formatters.shortDate.format(dayKeyToUtcDate(dateFilter))}
            {" · "}
            {notesLabel(selectedCount)}
          </span>
          <button
            type="button"
            onClick={() => setDateFilter(null)}
            className="shrink-0 rounded-[4px] text-[12px] text-[var(--cf-accent)] hover:underline focus-visible:underline focus-visible:outline-hidden"
          >
            <Trans>Any date</Trans>
          </button>
        </div>
      ) : null}
    </section>
  );
}
