import { Trans, useLingui } from "@lingui/react/macro";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";

import { CaretLeft, CaretRight, Check } from "@anlg/ui/components/icons";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@anlg/ui/components/ui/popover";
import { cn } from "@anlg/utils";

import { addDays, dayKeyToUtcDate, getWeekRange } from "./tasks-data";

import { useHumans } from "~/contacts/queries";

// Radix's default popover chrome is swapped for the C · Final glass panel.
const POPOVER_CONTENT_CLASS =
  "w-auto rounded-[16px] border-0 bg-transparent p-0 shadow-none";
const PANEL_CLASS = "cf-glass rounded-[16px] p-1.5 text-(--cf-label)";
const ITEM_CLASS =
  "cf-motion flex h-8 w-full min-w-0 items-center gap-2 rounded-[10px] px-2.5 text-left text-[13px] whitespace-nowrap transition-colors";

// ---------------------------------------------------------------------------
// Owner

export type OwnerOption = { id: string; name: string; email: string };

/** Contacts matching `query` by name or email, named ones first. */
export function filterOwnerOptions(
  options: OwnerOption[],
  query: string,
): OwnerOption[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return options;
  return options.filter(
    (option) =>
      option.name.toLocaleLowerCase().includes(needle) ||
      option.email.toLocaleLowerCase().includes(needle),
  );
}

export function OwnerPicker({
  ownerId,
  children,
  label,
  onSelect,
}: {
  ownerId: string;
  /** The single-line trigger content (the owner chip). */
  children: ReactNode;
  label: string;
  /** `humans.id`, or "" for "No owner". */
  onSelect: (humanId: string) => void;
}) {
  const [open, setOpen] = useState(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="block max-w-full min-w-0 cursor-pointer rounded-md text-left focus-visible:ring-2 focus-visible:ring-(--cf-accent) focus-visible:outline-none"
        >
          {children}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className={POPOVER_CONTENT_CLASS}>
        {open ? (
          <OwnerList
            ownerId={ownerId}
            onSelect={(humanId) => {
              setOpen(false);
              if (humanId !== ownerId) onSelect(humanId);
            }}
          />
        ) : null}
      </PopoverContent>
    </Popover>
  );
}

function OwnerList({
  ownerId,
  onSelect,
}: {
  ownerId: string;
  onSelect: (humanId: string) => void;
}) {
  const { t } = useLingui();
  const humans = useHumans();
  const listId = useId();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);

  const options = useMemo(
    () =>
      humans
        .filter((human) => human.name.trim() || human.email.trim())
        .map((human) => ({
          id: human.id,
          name: human.name.trim(),
          email: human.email.trim(),
        })),
    [humans],
  );
  const matches = useMemo(
    () => filterOwnerOptions(options, query).slice(0, 50),
    [options, query],
  );
  // "No owner" is always the first row, so it stays reachable while searching.
  const rows: Array<OwnerOption | null> = [null, ...matches];
  const active = Math.min(activeIndex, rows.length - 1);

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((active + step + rows.length) % rows.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      onSelect(rows[active]?.id ?? "");
    }
  };

  return (
    <div className={cn([PANEL_CLASS, "flex w-64 flex-col gap-1"])}>
      <input
        autoFocus
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setActiveIndex(matches.length > 0 && event.target.value ? 1 : 0);
        }}
        onKeyDown={handleKeyDown}
        placeholder={t`Search people`}
        aria-label={t`Search people`}
        role="combobox"
        aria-expanded
        aria-controls={listId}
        aria-activedescendant={`${listId}-${active}`}
        className="h-8 w-full min-w-0 rounded-[10px] border-0 bg-(--cf-fill) px-2.5 text-[13px] text-(--cf-label) outline-none placeholder:text-(--cf-secondary)"
      />
      <div
        id={listId}
        role="listbox"
        aria-label={t`People`}
        className="flex max-h-64 flex-col overflow-y-auto"
      >
        {rows.map((option, index) => {
          const id = option?.id ?? "";
          const selected = id === ownerId;
          return (
            <button
              key={id || "none"}
              id={`${listId}-${index}`}
              type="button"
              role="option"
              aria-selected={selected}
              tabIndex={-1}
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => onSelect(id)}
              className={cn([
                ITEM_CLASS,
                index === active && "cf-glass-sel",
                option ? "text-(--cf-label)" : "text-(--cf-secondary)",
              ])}
            >
              <span className="min-w-0 flex-1 truncate">
                {option ? (
                  <>
                    {option.name || option.email}
                    {option.name && option.email ? (
                      <span className="ml-1.5 text-(--cf-secondary)">
                        {option.email}
                      </span>
                    ) : null}
                  </>
                ) : (
                  <Trans>No owner</Trans>
                )}
              </span>
              {selected ? (
                <Check className="size-3.5 shrink-0 text-(--cf-accent)" />
              ) : null}
            </button>
          );
        })}
        {matches.length === 0 && query.trim() ? (
          <p className="px-2.5 py-1.5 text-[13px] text-(--cf-secondary)">
            <Trans>No matching people</Trans>
          </p>
        ) : null}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Due date

export function getQuickDueDays(
  todayKey: string,
  weekStartsOn: 0 | 1,
): { today: string; tomorrow: string; nextWeek: string } {
  return {
    today: todayKey,
    tomorrow: addDays(todayKey, 1),
    // First day of next week (Monday or Sunday, per the calendar setting).
    nextWeek: getWeekRange(todayKey, weekStartsOn).end,
  };
}

/** Day keys of the 6×7 grid that shows `monthKey` (`YYYY-MM`). */
export function getMonthGrid(monthKey: string, weekStartsOn: 0 | 1): string[] {
  const first = `${monthKey}-01`;
  const start = getWeekRange(first, weekStartsOn).start;
  return Array.from({ length: 42 }, (_, index) => addDays(start, index));
}

function shiftMonth(monthKey: string, months: number): string {
  const [year, month] = monthKey.split("-").map(Number);
  const date = new Date(Date.UTC(year!, month! - 1 + months, 1, 12));
  return date.toISOString().slice(0, 7);
}

export function DuePicker({
  dueDay,
  todayKey,
  weekStartsOn,
  children,
  label,
  onSelect,
}: {
  /** Current due day as `YYYY-MM-DD`, or null. */
  dueDay: string | null;
  todayKey: string;
  weekStartsOn: 0 | 1;
  children: ReactNode;
  label: string;
  /** `YYYY-MM-DD`, or "" to clear. */
  onSelect: (dueDay: string) => void;
}) {
  const [open, setOpen] = useState(false);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="block w-full min-w-0 cursor-pointer truncate rounded-md text-left focus-visible:ring-2 focus-visible:ring-(--cf-accent) focus-visible:outline-none"
        >
          {children}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className={POPOVER_CONTENT_CLASS}>
        {open ? (
          <DueCalendar
            dueDay={dueDay}
            todayKey={todayKey}
            weekStartsOn={weekStartsOn}
            onSelect={(day) => {
              setOpen(false);
              if (day !== (dueDay ?? "")) onSelect(day);
            }}
          />
        ) : null}
      </PopoverContent>
    </Popover>
  );
}

function DueCalendar({
  dueDay,
  todayKey,
  weekStartsOn,
  onSelect,
}: {
  dueDay: string | null;
  todayKey: string;
  weekStartsOn: 0 | 1;
  onSelect: (dueDay: string) => void;
}) {
  const { t, i18n } = useLingui();
  const [monthKey, setMonthKey] = useState(() =>
    (dueDay ?? todayKey).slice(0, 7),
  );
  const [focusKey, setFocusKey] = useState(() => dueDay ?? todayKey);
  const shouldMoveFocusRef = useRef(false);
  const gridRef = useRef<HTMLDivElement | null>(null);
  const quick = getQuickDueDays(todayKey, weekStartsOn);
  const days = getMonthGrid(monthKey, weekStartsOn);
  const locale = i18n.locale || undefined;
  const monthLabel = new Intl.DateTimeFormat(locale, {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  }).format(dayKeyToUtcDate(`${monthKey}-01`));
  const weekdayFormat = new Intl.DateTimeFormat(locale, {
    weekday: "narrow",
    timeZone: "UTC",
  });
  const dayLabelFormat = new Intl.DateTimeFormat(locale, {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });

  const weekdayLongFormat = new Intl.DateTimeFormat(locale, {
    weekday: "long",
    timeZone: "UTC",
  });

  // Roving tabindex: one tab stop in the grid, arrow keys move between days.
  const tabStopKey = days.includes(focusKey)
    ? focusKey
    : dueDay && dueDay.startsWith(monthKey)
      ? dueDay
      : todayKey.startsWith(monthKey)
        ? todayKey
        : `${monthKey}-01`;

  useEffect(() => {
    if (!shouldMoveFocusRef.current) return;
    shouldMoveFocusRef.current = false;
    gridRef.current
      ?.querySelector<HTMLElement>(`[data-day="${focusKey}"]`)
      ?.focus();
  }, [focusKey, monthKey]);

  const changeMonth = (delta: number) => {
    const next = shiftMonth(monthKey, delta);
    setMonthKey(next);
    setFocusKey(`${next}-01`);
  };

  const handleGridKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const current = tabStopKey;
    const weekday =
      (dayKeyToUtcDate(current).getUTCDay() - weekStartsOn + 7) % 7;
    const steps: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -7,
      ArrowDown: 7,
      Home: -weekday,
      End: 6 - weekday,
    };
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(current);
      return;
    }
    const step = steps[event.key];
    if (step === undefined) return;
    event.preventDefault();
    const next = addDays(current, step);
    shouldMoveFocusRef.current = true;
    setFocusKey(next);
    if (!next.startsWith(monthKey)) setMonthKey(next.slice(0, 7));
  };

  const quickPicks: Array<{ day: string; label: string }> = [
    { day: quick.today, label: t`Today` },
    { day: quick.tomorrow, label: t`Tomorrow` },
    { day: quick.nextWeek, label: t`Next week` },
  ];

  return (
    <div className={cn([PANEL_CLASS, "flex w-64 flex-col gap-1"])}>
      <div className="flex flex-col">
        {quickPicks.map((pick) => (
          <button
            key={pick.label}
            type="button"
            onClick={() => onSelect(pick.day)}
            className={cn([ITEM_CLASS, "hover:bg-(--cf-glass-sel)"])}
          >
            <span className="flex-1">{pick.label}</span>
            <span className="text-(--cf-secondary)">
              {new Intl.DateTimeFormat(locale, {
                weekday: "short",
                month: "short",
                day: "numeric",
                timeZone: "UTC",
              }).format(dayKeyToUtcDate(pick.day))}
            </span>
          </button>
        ))}
      </div>
      <div className="mx-1 h-px bg-(--cf-separator)" />
      <div className="flex h-8 items-center justify-between px-1">
        <span className="truncate pl-1.5 text-[13px] font-semibold">
          {monthLabel}
        </span>
        <span className="flex shrink-0 items-center">
          <button
            type="button"
            onClick={() => changeMonth(-1)}
            aria-label={t`Previous month`}
            className="cf-motion flex size-7 items-center justify-center rounded-full text-(--cf-secondary) transition-colors hover:bg-(--cf-fill) hover:text-(--cf-label)"
          >
            <CaretLeft className="size-3.5" />
          </button>
          <button
            type="button"
            onClick={() => changeMonth(1)}
            aria-label={t`Next month`}
            className="cf-motion flex size-7 items-center justify-center rounded-full text-(--cf-secondary) transition-colors hover:bg-(--cf-fill) hover:text-(--cf-label)"
          >
            <CaretRight className="size-3.5" />
          </button>
        </span>
      </div>
      <div
        ref={gridRef}
        role="grid"
        aria-label={monthLabel}
        onKeyDown={handleGridKeyDown}
        className="flex flex-col gap-0.5 px-0.5 text-center text-xs"
      >
        <div role="row" className="grid grid-cols-7">
          {days.slice(0, 7).map((day) => (
            <span
              key={`weekday-${day}`}
              role="columnheader"
              aria-label={weekdayLongFormat.format(dayKeyToUtcDate(day))}
              className="flex h-6 items-center justify-center text-[11px] text-(--cf-secondary)"
            >
              {weekdayFormat.format(dayKeyToUtcDate(day))}
            </span>
          ))}
        </div>
        {Array.from({ length: 6 }, (_, week) =>
          days.slice(week * 7, week * 7 + 7),
        ).map((weekDays) => (
          <div key={weekDays[0]} role="row" className="grid grid-cols-7">
            {weekDays.map((day) => {
              const selected = day === dueDay;
              const isToday = day === todayKey;
              const inMonth = day.startsWith(monthKey);
              return (
                <span
                  key={day}
                  role="gridcell"
                  aria-selected={selected}
                  aria-current={isToday ? "date" : undefined}
                  aria-label={dayLabelFormat.format(dayKeyToUtcDate(day))}
                  data-day={day}
                  tabIndex={day === tabStopKey ? 0 : -1}
                  onClick={() => onSelect(day)}
                  onFocus={() => setFocusKey(day)}
                  className={cn([
                    "cf-motion mx-auto flex size-8 cursor-default items-center justify-center rounded-full tabular-nums transition-colors select-none",
                    "focus-visible:ring-2 focus-visible:ring-(--cf-accent) focus-visible:outline-hidden",
                    selected
                      ? "cf-glass-tint font-semibold"
                      : cn([
                          "hover:bg-(--cf-fill)",
                          isToday && "font-semibold text-(--cf-accent)",
                          !isToday &&
                            (inMonth
                              ? "text-(--cf-label)"
                              : "text-(--cf-tertiary)"),
                        ]),
                  ])}
                >
                  {Number(day.slice(8))}
                </span>
              );
            })}
          </div>
        ))}
      </div>
      {dueDay ? (
        <>
          <div className="mx-1 h-px bg-(--cf-separator)" />
          <button
            type="button"
            onClick={() => onSelect("")}
            className={cn([
              ITEM_CLASS,
              "text-(--cf-secondary) hover:bg-(--cf-glass-sel) hover:text-(--cf-label)",
            ])}
          >
            <Trans>Clear</Trans>
          </button>
        </>
      ) : null}
    </div>
  );
}
