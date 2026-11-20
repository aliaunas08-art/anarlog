import { useLiveQuery } from "~/db";
import { resolvedTaskSessionIdSql } from "~/editor-bridge/task-storage";
import { toTimelineDayKey } from "~/sidebar/timeline/utils";

/**
 * One row of the Tasks database: an `action_items` row joined to its meeting
 * (`sessions`) and, when set, its assignee (`humans` via `assignee_human_id`).
 * Owner and due date are only shown when those columns actually hold a value.
 */
export type TaskItem = {
  id: string;
  text: string;
  status: string;
  done: boolean;
  dueAt: string;
  ownerId: string;
  ownerName: string;
  sessionId: string;
  sessionTitle: string;
  sessionDate: string;
  sourceOrder: number;
  doneAt: string;
};

export type TaskRow = {
  id: string;
  status: string;
  text: string;
  due_at: string | null;
  assignee_human_id: string | null;
  owner_name: string | null;
  owner_email: string | null;
  session_id: string | null;
  session_title: string | null;
  session_date: string | null;
  source_order: number | null;
  done_at: string | null;
};

const DONE_STATUSES_SQL = `('done', 'completed')`;

/** Done items older than the most recent ones aren't loaded. */
export const DONE_TASK_LIMIT = 200;

// Rows whose meeting (or source summary) was deleted are hidden; standalone
// rows (no session) stay. Editor rows may have an empty `session_id`, so the
// meeting is resolved from the row's source.
const VISIBLE_TASKS_FROM_SQL = `
  FROM (
    SELECT
      item.id,
      item.status,
      item.text,
      item.due_at,
      item.assignee_human_id,
      item.source_type,
      item.source_id,
      item.source_order,
      item.completed_at,
      item.updated_at,
      ${resolvedTaskSessionIdSql("item")} AS session_id
    FROM action_items AS item
    WHERE item.deleted_at IS NULL
      AND TRIM(item.text) <> ''
  ) AS item
  LEFT JOIN sessions AS session
    ON session.id = item.session_id
    AND item.session_id <> ''
    AND session.deleted_at IS NULL
  WHERE (item.session_id = '' OR session.id IS NOT NULL)
    AND (
      item.source_type <> 'enhanced_note'
      OR EXISTS (
        SELECT 1
        FROM session_documents AS source_document
        WHERE source_document.id = item.source_id
          AND source_document.deleted_at IS NULL
      )
    )
`;

const VISIBLE_TASK_COLUMNS_SQL = `
  item.id,
  item.status,
  item.text,
  item.due_at,
  item.assignee_human_id,
  item.session_id,
  session.title AS session_title,
  COALESCE(NULLIF(session.started_at, ''), session.created_at, '') AS session_date,
  item.source_order,
  COALESCE(NULLIF(item.completed_at, ''), item.updated_at, '') AS done_at
`;

// Open items are all loaded; done items only the most recent ones.
export const TASKS_SQL = `
  SELECT
    task.id,
    task.status,
    task.text,
    task.due_at,
    task.assignee_human_id,
    human.name AS owner_name,
    human.email AS owner_email,
    task.session_id,
    task.session_title,
    task.session_date,
    task.source_order,
    task.done_at
  FROM (
    SELECT ${VISIBLE_TASK_COLUMNS_SQL}
    ${VISIBLE_TASKS_FROM_SQL}
      AND item.status NOT IN ${DONE_STATUSES_SQL}
    UNION ALL
    SELECT * FROM (
      SELECT ${VISIBLE_TASK_COLUMNS_SQL}
      ${VISIBLE_TASKS_FROM_SQL}
        AND item.status IN ${DONE_STATUSES_SQL}
      ORDER BY done_at DESC, item.id
      LIMIT ${DONE_TASK_LIMIT}
    )
  ) AS task
  LEFT JOIN humans AS human
    ON human.id = task.assignee_human_id
    AND task.assignee_human_id <> ''
    AND human.deleted_at IS NULL
  ORDER BY task.session_date DESC, task.session_id, task.source_order, task.id
`;

export const OPEN_TASK_COUNT_SQL = `
  SELECT COUNT(*) AS count
  ${VISIBLE_TASKS_FROM_SQL}
    AND item.status NOT IN ${DONE_STATUSES_SQL}
`;

export function isDoneStatus(status: string): boolean {
  return status === "done" || status === "completed";
}

export function mapTaskRows(rows: TaskRow[]): TaskItem[] {
  return rows.map((row) => ({
    id: row.id,
    text: (row.text ?? "").trim(),
    status: row.status,
    done: isDoneStatus(row.status),
    dueAt: (row.due_at ?? "").trim(),
    ownerId: row.assignee_human_id ?? "",
    ownerName: (row.owner_name || row.owner_email || "").trim(),
    sessionId: row.session_id ?? "",
    sessionTitle: (row.session_title ?? "").trim(),
    sessionDate: row.session_date ?? "",
    sourceOrder: Number(row.source_order ?? 0),
    doneAt: row.done_at ?? "",
  }));
}

const EMPTY_TASKS: TaskItem[] = [];

export function useTaskItems() {
  const { data, isLoading } = useLiveQuery<TaskRow, TaskItem[]>({
    sql: TASKS_SQL,
    mapRows: mapTaskRows,
  });
  return { tasks: data ?? EMPTY_TASKS, isLoading };
}

/** Live number of open (not done) action items, e.g. for "Tasks · 6 open". */
export function useOpenTaskCount(): number {
  const { data } = useLiveQuery<{ count: number | null }, number>({
    sql: OPEN_TASK_COUNT_SQL,
    mapRows: (rows) => Number(rows[0]?.count ?? 0),
  });
  return data ?? 0;
}

// ---------------------------------------------------------------------------
// Due dates & grouping (pure helpers, unit tested)
//
// Dates are compared as `YYYY-MM-DD` day keys in the configured timezone, the
// same rule the sidebar timeline and date filter use.

export type TaskDateContext = {
  /** Today as a `YYYY-MM-DD` key in `timezone`. */
  todayKey: string;
  weekStartsOn: 0 | 1;
  timezone?: string;
};

const DAY_MS = 24 * 60 * 60 * 1000;

/** Noon UTC of the day: formatting with timeZone "UTC" never shifts the day. */
export function dayKeyToUtcDate(dayKey: string): Date {
  const [year, month, day] = dayKey.split("-").map(Number);
  return new Date(Date.UTC(year!, month! - 1, day!, 12));
}

function toUtcDayKey(date: Date): string {
  return date.toISOString().slice(0, 10);
}

export function addDays(dayKey: string, days: number): string {
  return toUtcDayKey(
    new Date(dayKeyToUtcDate(dayKey).getTime() + days * DAY_MS),
  );
}

export function diffDays(fromKey: string, toKey: string): number {
  return Math.round(
    (dayKeyToUtcDate(toKey).getTime() - dayKeyToUtcDate(fromKey).getTime()) /
      DAY_MS,
  );
}

/**
 * `due_at` is free-form TEXT in the schema. Accept a plain `YYYY-MM-DD` (a
 * calendar day) or any ISO timestamp (its day in `timezone`); anything else is
 * treated as "no date" so we never guess.
 */
export function getDueDayKey(dueAt: string, timezone?: string): string | null {
  if (!dueAt) return null;
  if (/^\d{4}-\d{2}-\d{2}$/.test(dueAt)) {
    return toUtcDayKey(dayKeyToUtcDate(dueAt)) === dueAt ? dueAt : null;
  }
  if (!/^\d{4}-\d{2}-\d{2}T/.test(dueAt)) return null;
  const parsed = new Date(dueAt);
  return Number.isNaN(parsed.getTime())
    ? null
    : toTimelineDayKey(parsed, timezone);
}

/** First day of the week containing `todayKey` → first day of the next week. */
export function getWeekRange(
  todayKey: string,
  weekStartsOn: 0 | 1,
): { start: string; end: string } {
  const weekday = dayKeyToUtcDate(todayKey).getUTCDay();
  const start = addDays(todayKey, -((weekday - weekStartsOn + 7) % 7));
  return { start, end: addDays(start, 7) };
}

export type DueBucket = "this_week" | "later" | "no_date";

/** Overdue items count as "this week" so they stay in sight. */
export function getDueBucket(
  dueAt: string,
  context: TaskDateContext,
): DueBucket {
  const due = getDueDayKey(dueAt, context.timezone);
  if (!due) return "no_date";
  return due < getWeekRange(context.todayKey, context.weekStartsOn).end
    ? "this_week"
    : "later";
}

export type TaskView = "all" | "person" | "meeting" | "done";

export type TaskGroup = {
  key: string;
  kind: "due" | "person" | "meeting" | "done";
  /** Due bucket, owner name or meeting title (empty = unassigned/untitled). */
  label: string;
  bucket?: DueBucket;
  sessionId?: string;
  tasks: TaskItem[];
};

function compareDue(timezone: string | undefined) {
  return (a: TaskItem, b: TaskItem): number => {
    const left = getDueDayKey(a.dueAt, timezone);
    const right = getDueDayKey(b.dueAt, timezone);
    if (left === right) return 0;
    if (left === null) return 1;
    if (right === null) return -1;
    return left < right ? -1 : 1;
  };
}

export function matchesTaskQuery(task: TaskItem, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    task.text.toLowerCase().includes(q) ||
    task.ownerName.toLowerCase().includes(q) ||
    task.sessionTitle.toLowerCase().includes(q)
  );
}

export function groupTasks(
  tasks: TaskItem[],
  view: TaskView,
  context: TaskDateContext,
): TaskGroup[] {
  const byDue = compareDue(context.timezone);
  if (view === "done") {
    const done = tasks
      .filter((task) => task.done)
      .sort((a, b) => b.doneAt.localeCompare(a.doneAt));
    return done.length > 0
      ? [{ key: "done", kind: "done", label: "", tasks: done }]
      : [];
  }

  const open = tasks.filter((task) => !task.done);

  if (view === "all") {
    const buckets: DueBucket[] = ["this_week", "later", "no_date"];
    return buckets
      .map(
        (bucket): TaskGroup => ({
          key: `due:${bucket}`,
          kind: "due",
          label: bucket,
          bucket,
          tasks: open
            .filter((task) => getDueBucket(task.dueAt, context) === bucket)
            .sort(byDue),
        }),
      )
      .filter((group) => group.tasks.length > 0);
  }

  if (view === "person") {
    const byOwner = new Map<string, TaskGroup>();
    for (const task of open) {
      const key = `person:${task.ownerName ? task.ownerId || task.ownerName : ""}`;
      const group = byOwner.get(key) ?? {
        key,
        kind: "person" as const,
        label: task.ownerName,
        tasks: [],
      };
      group.tasks.push(task);
      byOwner.set(key, group);
    }
    return [...byOwner.values()]
      .map((group) => ({ ...group, tasks: group.tasks.sort(byDue) }))
      .sort((a, b) => {
        // Unassigned last, otherwise alphabetical.
        if (!a.label) return 1;
        if (!b.label) return -1;
        return a.label.localeCompare(b.label);
      });
  }

  // By meeting: keeps the query's newest-meeting-first order.
  const byMeeting = new Map<string, TaskGroup>();
  for (const task of open) {
    const key = `meeting:${task.sessionId}`;
    const group = byMeeting.get(key) ?? {
      key,
      kind: "meeting" as const,
      label: task.sessionTitle,
      sessionId: task.sessionId,
      tasks: [],
    };
    group.tasks.push(task);
    byMeeting.set(key, group);
  }
  return [...byMeeting.values()];
}
