import { Trans, useLingui } from "@lingui/react/macro";
import { motion, useReducedMotion } from "motion/react";
import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useCallback,
  useMemo,
  useRef,
  useState,
} from "react";

import type { TaskStatus } from "@anlg/editor/tasks";
import { ListChecks, MagnifyingGlass } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { DuePicker, OwnerPicker } from "./task-cell-pickers";
import {
  addDays,
  dayKeyToUtcDate,
  diffDays,
  getDueDayKey,
  getWeekRange,
  groupTasks,
  matchesTaskQuery,
  type TaskDateContext,
  type TaskGroup,
  type TaskItem,
  type TaskView,
  useTaskItems,
} from "./tasks-data";

import { useTimezone, useWeekStartsOn } from "~/calendar/hooks";
import {
  setActionItemDue,
  setActionItemOwner,
  setActionItemStatus,
} from "~/editor-bridge/task-storage";
import { useCurrentDay } from "~/shared/hooks/useCurrentDay";
import { StandardContentWrapper } from "~/shared/main";
import { useOwnerUserId } from "~/shared/owner-user";
import { useTabs } from "~/store/zustand/tabs";

type Toggled = Map<
  string,
  {
    groupAsDone: boolean;
    done: boolean;
    /** Status to restore when unchecking a task checked off in this view. */
    previousStatus: TaskStatus;
  }
>;

function toOpenStatus(status: string): TaskStatus {
  return status === "in_progress" ? "in_progress" : "todo";
}

export function TabContentTasks() {
  const [view, setView] = useState<TaskView>("all");

  return (
    <StandardContentWrapper
      floatingButton={
        <div className="pointer-events-none absolute inset-x-0 bottom-5 z-20 flex justify-center px-4">
          <TaskViewSwitcher value={view} onChange={setView} />
        </div>
      }
    >
      <TasksView view={view} />
    </StandardContentWrapper>
  );
}

export function TasksView({ view }: { view: TaskView }) {
  const { t, i18n } = useLingui();
  const { tasks, isLoading } = useTaskItems();
  const ownerUserId = useOwnerUserId();
  const openCurrent = useTabs((state) => state.openCurrent);
  const [query, setQuery] = useState("");
  // Rows toggled in this view stay where they were (struck through) until the
  // view changes, so checking one off doesn't make the list jump.
  const [toggled, setToggled] = useState<{ view: TaskView; map: Toggled }>({
    view,
    map: new Map(),
  });
  const toggledMap = toggled.view === view ? toggled.map : null;
  const timezone = useTimezone();
  const weekStartsOn = useWeekStartsOn();
  const todayKey = useCurrentDay(timezone);
  const dateContext = useMemo<TaskDateContext>(
    () => ({ todayKey, weekStartsOn, timezone }),
    [todayKey, weekStartsOn, timezone],
  );

  const groups = useMemo(() => {
    const visible = tasks
      .filter((task) => matchesTaskQuery(task, query))
      .map((task) => {
        const entry = toggledMap?.get(task.id);
        return entry ? { ...task, done: entry.groupAsDone } : task;
      });
    return groupTasks(visible, view, dateContext);
  }, [tasks, query, toggledMap, view, dateContext]);

  const handleToggle = useCallback(
    (task: TaskItem, displayedDone: boolean) => {
      const nextDone = !displayedDone;
      const entry = toggledMap?.get(task.id);
      const previousStatus =
        entry?.previousStatus ??
        (task.done ? "todo" : toOpenStatus(task.status));
      setToggled((prev) => {
        const map = new Map(prev.view === view ? prev.map : undefined);
        const groupAsDone = map.get(task.id)?.groupAsDone ?? task.done;
        map.set(task.id, { groupAsDone, done: nextDone, previousStatus });
        return { view, map };
      });
      void setActionItemStatus(
        task.id,
        nextDone ? "done" : previousStatus,
        ownerUserId ?? undefined,
      );
    },
    [ownerUserId, toggledMap, view],
  );

  const handleOwner = useCallback(
    (taskId: string, humanId: string) => {
      void setActionItemOwner(taskId, humanId, ownerUserId ?? undefined);
    },
    [ownerUserId],
  );
  const handleDue = useCallback(
    (taskId: string, dueDay: string) => {
      void setActionItemDue(taskId, dueDay, ownerUserId ?? undefined);
    },
    [ownerUserId],
  );

  const openMeeting = useCallback(
    (sessionId: string) => {
      if (sessionId) openCurrent({ type: "sessions", id: sessionId });
    },
    [openCurrent],
  );

  // Day keys are formatted at noon UTC so the calendar day never shifts;
  // meeting timestamps are formatted in the configured timezone.
  const formatDay = useCallback(
    (dayKey: string, withWeekday = true) =>
      new Intl.DateTimeFormat(i18n.locale || undefined, {
        ...(withWeekday ? { weekday: "short" } : {}),
        month: "short",
        day: "numeric",
        timeZone: "UTC",
      }).format(dayKeyToUtcDate(dayKey)),
    [i18n.locale],
  );
  const formatInstant = useCallback(
    (date: Date) =>
      new Intl.DateTimeFormat(i18n.locale || undefined, {
        month: "short",
        day: "numeric",
        timeZone: timezone,
      }).format(date),
    [i18n.locale, timezone],
  );

  const week = getWeekRange(todayKey, weekStartsOn);
  const weekLast = addDays(week.end, -1);
  const weekLabel = `${formatDay(week.start, false)} – ${
    week.start.slice(0, 7) === weekLast.slice(0, 7)
      ? Number(weekLast.slice(8))
      : formatDay(weekLast, false)
  }`;

  const groupHeading = (group: TaskGroup): ReactNode => {
    switch (group.kind) {
      case "due":
        if (group.bucket === "this_week") {
          return (
            <>
              <Trans>This week</Trans>
              <GroupMeta>· {weekLabel}</GroupMeta>
            </>
          );
        }
        return group.bucket === "later" ? (
          <Trans>Later</Trans>
        ) : (
          <Trans>No date</Trans>
        );
      case "person":
        return group.label || <Trans>No owner</Trans>;
      case "meeting": {
        const date = group.tasks[0]?.sessionDate
          ? new Date(group.tasks[0].sessionDate)
          : null;
        return (
          <>
            {group.sessionId ? (
              <button
                type="button"
                onClick={() => openMeeting(group.sessionId ?? "")}
                className="cf-motion min-w-0 truncate uppercase transition-colors hover:text-(--cf-accent)"
              >
                {group.label || t`Untitled meeting`}
              </button>
            ) : (
              <Trans>Not from a meeting</Trans>
            )}
            {date && !Number.isNaN(date.getTime()) ? (
              <GroupMeta>· {formatInstant(date)}</GroupMeta>
            ) : null}
          </>
        );
      }
      case "done":
        return (
          <>
            <Trans>Done</Trans>
            <GroupMeta>· {group.tasks.length}</GroupMeta>
          </>
        );
    }
  };

  const formatDue = (task: TaskItem) => {
    const due = getDueDayKey(task.dueAt, timezone);
    if (!due) {
      return {
        day: null,
        label: task.dueAt ? task.dueAt : "—",
        overdue: false,
      };
    }
    const days = diffDays(todayKey, due);
    const label =
      days === 0
        ? t`Today`
        : days === 1
          ? t`Tomorrow`
          : days === -1
            ? t`Yesterday`
            : formatDay(due);
    return { day: due, label, overdue: days < 0 };
  };

  const hasAnyTasks = tasks.length > 0;

  return (
    <div className="relative h-full overflow-auto [scrollbar-gutter:stable]">
      <div className="mx-auto flex max-w-[1040px] flex-col gap-5 px-8 pt-8 pb-32 text-(--cf-label)">
        <header className="flex flex-wrap items-end justify-between gap-3">
          <div className="flex flex-col gap-1.5">
            <p className="text-xs font-semibold tracking-[0.06em] text-(--cf-secondary) uppercase">
              <Trans>From every meeting</Trans>
            </p>
            <h1 className="text-[40px] leading-none font-bold tracking-[-0.035em]">
              <Trans>Tasks</Trans>
            </h1>
          </div>
          {hasAnyTasks ? (
            <label className="rounded-pill flex h-8 w-52 items-center gap-1.5 bg-(--cf-fill) px-3 text-(--cf-secondary)">
              <MagnifyingGlass className="size-3.5 shrink-0" />
              <input
                type="search"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t`Search tasks`}
                aria-label={t`Search tasks`}
                className="min-w-0 flex-1 border-0 bg-transparent text-[13px] text-(--cf-label) outline-none placeholder:text-(--cf-secondary)"
              />
            </label>
          ) : null}
        </header>

        {isLoading && !hasAnyTasks ? null : groups.length === 0 ? (
          <TasksEmptyState
            kind={
              !hasAnyTasks
                ? "none"
                : query.trim()
                  ? "search"
                  : view === "done"
                    ? "no_done"
                    : "all_done"
            }
            query={query.trim()}
          />
        ) : (
          <table className="w-full table-fixed border-collapse text-sm">
            <colgroup>
              <col className="w-8" />
              <col />
              <col className="w-[18%] min-w-24" />
              <col className="w-[14%]" />
              <col className="w-[26%]" />
            </colgroup>
            <thead>
              <tr className="text-left text-xs text-(--cf-secondary)">
                <th className="border-b border-(--cf-separator) py-2 font-medium">
                  <span className="sr-only">
                    <Trans>Done</Trans>
                  </span>
                </th>
                <th className="border-b border-(--cf-separator) py-2 pr-3 font-medium">
                  <Trans>Task</Trans>
                </th>
                <th className="border-b border-(--cf-separator) px-3 py-2 font-medium">
                  <Trans>Owner</Trans>
                </th>
                <th className="border-b border-(--cf-separator) px-3 py-2 font-medium">
                  <Trans>Due</Trans>
                </th>
                <th className="border-b border-(--cf-separator) px-3 py-2 font-medium">
                  <Trans>From meeting</Trans>
                </th>
              </tr>
            </thead>
            {groups.map((group, groupIndex) => (
              <tbody key={group.key} data-testid={`task-group-${group.key}`}>
                <tr>
                  <th
                    colSpan={5}
                    scope="colgroup"
                    className={cn([
                      "pb-1.5 text-left text-xs font-bold tracking-[0.06em] uppercase",
                      groupIndex === 0 ? "pt-4" : "pt-6",
                    ])}
                  >
                    <span className="flex min-w-0 items-baseline gap-1">
                      {groupHeading(group)}
                    </span>
                  </th>
                </tr>
                {group.tasks.map((task) => {
                  const displayedDone =
                    toggledMap?.get(task.id)?.done ?? task.done;
                  const due = formatDue(task);
                  return (
                    <tr
                      key={task.id}
                      data-testid="task-row"
                      className="group/row h-10"
                    >
                      <td className="border-b border-(--cf-separator) py-0 align-middle">
                        <input
                          type="checkbox"
                          checked={displayedDone}
                          onChange={() => handleToggle(task, displayedDone)}
                          aria-label={task.text}
                          className="block size-4 cursor-pointer accent-(--cf-accent)"
                        />
                      </td>
                      <td className="border-b border-(--cf-separator) py-0 pr-3 align-middle">
                        <span
                          title={task.text}
                          className={cn([
                            "cf-motion block truncate font-medium transition-colors",
                            displayedDone &&
                              "text-(--cf-tertiary) line-through decoration-(--cf-tertiary)",
                          ])}
                        >
                          {task.text}
                        </span>
                      </td>
                      <td className="border-b border-(--cf-separator) px-3 py-0 align-middle">
                        <OwnerPicker
                          ownerId={task.ownerId}
                          label={t`Owner of ${task.text}: ${
                            task.ownerName || t`No owner`
                          }`}
                          onSelect={(humanId) => handleOwner(task.id, humanId)}
                        >
                          <span
                            title={task.ownerName || undefined}
                            className={cn([
                              "cf-motion block max-w-full truncate rounded-md bg-(--cf-fill) px-2 py-0.5 text-xs transition-colors hover:bg-(--cf-glass-sel)",
                              task.ownerName
                                ? "text-(--cf-label)"
                                : "text-(--cf-secondary)",
                            ])}
                          >
                            {task.ownerName || <Trans>No owner</Trans>}
                          </span>
                        </OwnerPicker>
                      </td>
                      <td
                        className={cn([
                          "truncate border-b border-(--cf-separator) px-3 py-0 align-middle",
                          due.overdue && !displayedDone
                            ? "text-(--cf-record)"
                            : "text-(--cf-secondary)",
                        ])}
                      >
                        <DuePicker
                          dueDay={due.day}
                          todayKey={todayKey}
                          weekStartsOn={weekStartsOn}
                          label={t`Due date of ${task.text}: ${
                            due.day || task.dueAt ? due.label : t`No date`
                          }`}
                          onSelect={(dueDay) => handleDue(task.id, dueDay)}
                        >
                          <span className="cf-motion transition-colors hover:text-(--cf-label)">
                            {due.label}
                          </span>
                        </DuePicker>
                      </td>
                      <td className="border-b border-(--cf-separator) px-3 py-0 align-middle">
                        {task.sessionId ? (
                          <button
                            type="button"
                            onClick={() => openMeeting(task.sessionId)}
                            title={task.sessionTitle || t`Untitled meeting`}
                            className="cf-motion block max-w-full truncate text-left underline decoration-(--cf-separator) underline-offset-[3px] transition-colors hover:text-(--cf-accent) hover:decoration-current"
                          >
                            {task.sessionTitle || t`Untitled meeting`}
                          </button>
                        ) : (
                          <span className="text-(--cf-secondary)">—</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            ))}
          </table>
        )}
      </div>
    </div>
  );
}

function GroupMeta({ children }: { children: ReactNode }) {
  return (
    <span className="shrink-0 font-medium tracking-normal text-(--cf-secondary) normal-case">
      {children}
    </span>
  );
}

function TasksEmptyState({
  kind,
  query,
}: {
  kind: "none" | "search" | "no_done" | "all_done";
  query: string;
}) {
  return (
    <div
      data-testid="tasks-empty"
      className="flex flex-col items-center gap-2 py-24 text-center"
    >
      <div className="rounded-pill mb-2 flex size-12 items-center justify-center bg-(--cf-fill) text-(--cf-secondary)">
        <ListChecks className="size-5" />
      </div>
      <p className="text-[15px] font-semibold text-(--cf-label)">
        {kind === "none" ? (
          <Trans>No tasks yet</Trans>
        ) : kind === "search" ? (
          <Trans>No tasks match “{query}”</Trans>
        ) : kind === "no_done" ? (
          <Trans>Nothing checked off yet</Trans>
        ) : (
          <Trans>You’re all caught up</Trans>
        )}
      </p>
      <p className="max-w-sm text-[13px] text-(--cf-secondary)">
        {kind === "none" ? (
          <Trans>
            Add a to-do in any meeting note and it will show up here, together
            with the meeting it came from.
          </Trans>
        ) : kind === "search" ? (
          <Trans>Try a different word, a person or a meeting title.</Trans>
        ) : kind === "no_done" ? (
          <Trans>Tasks you check off will collect here.</Trans>
        ) : (
          <Trans>Every task from your meetings is done.</Trans>
        )}
      </p>
    </div>
  );
}

export function TaskViewSwitcher({
  value,
  onChange,
}: {
  value: TaskView;
  onChange: (view: TaskView) => void;
}) {
  const { t } = useLingui();
  const reduceMotion = useReducedMotion();
  const buttonsRef = useRef<Array<HTMLButtonElement | null>>([]);
  const options: Array<{ value: TaskView; label: string }> = [
    { value: "all", label: t`All` },
    { value: "person", label: t`By person` },
    { value: "meeting", label: t`By meeting` },
    { value: "done", label: t`Done` },
  ];
  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );

  // Radio group pattern: one tab stop, arrows move and select, Home/End jump.
  const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const last = options.length - 1;
    const nextIndex =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? selectedIndex === last
          ? 0
          : selectedIndex + 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? selectedIndex === 0
            ? last
            : selectedIndex - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (nextIndex === null) return;
    event.preventDefault();
    onChange(options[nextIndex]!.value);
    buttonsRef.current[nextIndex]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={t`Task views`}
      onKeyDown={handleKeyDown}
      className="cf-seg rounded-pill pointer-events-auto flex items-center gap-0.5 p-[5px]"
    >
      {options.map((option, index) => {
        const selected = index === selectedIndex;
        return (
          <button
            key={option.value}
            ref={(element) => {
              buttonsRef.current[index] = element;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(option.value)}
            className={cn([
              "cf-motion rounded-pill relative h-9 px-4 text-[13px] whitespace-nowrap transition-colors",
              selected
                ? "font-semibold text-(--cf-window)"
                : "text-(--cf-secondary) hover:text-(--cf-label)",
            ])}
          >
            {selected ? (
              <motion.span
                aria-hidden="true"
                layoutId="task-view-thumb"
                className="cf-bar-thumb rounded-pill absolute inset-0"
                transition={
                  reduceMotion
                    ? { duration: 0 }
                    : { type: "spring", bounce: 0, duration: 0.3 }
                }
              />
            ) : null}
            <span className="relative">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
