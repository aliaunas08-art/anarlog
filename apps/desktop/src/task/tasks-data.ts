import { useLiveQuery } from "~/db";

/**
 * One row of the Tasks database: an `action_items` row joined to its meeting
 * (`sessions`) and, when set, its assignee (`humans` via `assignee_human_id`).
 * Owner and due date are only shown when those columns actually hold a value.
 */
export type TaskItem = {
  id: string;
  text: string;
  done: boolean;
  dueAt: string;
  ownerId: string;
  ownerName: string;
  sessionId: string;
  sessionTitle: string;
  sessionDate: string;
  sourceOrder: number;
  updatedAt: string;
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
  updated_at: string | null;
};

// Rows whose meeting was deleted are hidden; standalone rows (no session) stay.
const TASK_FROM_SQL = `
  FROM action_items AS item
  LEFT JOIN sessions AS session
    ON session.id = item.session_id
    AND item.session_id <> ''
    AND session.deleted_at IS NULL
  WHERE item.deleted_at IS NULL
    AND TRIM(item.text) <> ''
    AND (item.session_id = '' OR session.id IS NOT NULL)
`;

const DONE_STATUSES_SQL = `('done', 'completed')`;

export const TASKS_SQL = `
  SELECT
    item.id,
    item.status,
    item.text,
    item.due_at,
    item.assignee_human_id,
    human.name AS owner_name,
    human.email AS owner_email,
    item.session_id,
    session.title AS session_title,
    COALESCE(NULLIF(session.started_at, ''), session.created_at, '') AS session_date,
    item.source_order,
    item.updated_at
  FROM action_items AS item
  LEFT JOIN sessions AS session
    ON session.id = item.session_id
    AND item.session_id <> ''
    AND session.deleted_at IS NULL
  LEFT JOIN humans AS human
    ON human.id = item.assignee_human_id
    AND item.assignee_human_id <> ''
    AND human.deleted_at IS NULL
  WHERE item.deleted_at IS NULL
    AND TRIM(item.text) <> ''
    AND (item.session_id = '' OR session.id IS NOT NULL)
  ORDER BY session_date DESC, item.session_id, item.source_order, item.id
`;

export const OPEN_TASK_COUNT_SQL = `
  SELECT COUNT(*) AS count
  ${TASK_FROM_SQL}
    AND item.status NOT IN ${DONE_STATUSES_SQL}
`;

export function isDoneStatus(status: string): boolean {
  return status === "done" || status === "completed";
}

export function mapTaskRows(rows: TaskRow[]): TaskItem[] {
  return rows.map((row) => ({
    id: row.id,
    text: (row.text ?? "").trim(),
    done: isDoneStatus(row.status),
    dueAt: (row.due_at ?? "").trim(),
    ownerId: row.assignee_human_id ?? "",
    ownerName: (row.owner_name || row.owner_email || "").trim(),
    sessionId: row.session_id ?? "",
    sessionTitle: (row.session_title ?? "").trim(),
    sessionDate: row.session_date ?? "",
    sourceOrder: Number(row.source_order ?? 0),
    updatedAt: row.updated_at ?? "",
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

/**
 * `due_at` is free-form TEXT in the schema. Accept a plain `YYYY-MM-DD` (as a
 * local calendar day) or any ISO timestamp; anything else is treated as
 * "no date" so we never guess.
 */
export function parseDueDate(dueAt: string): Date | null {
  if (!dueAt) return null;
  const dayOnly = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dueAt);
  if (dayOnly) {
    const [, y, m, d] = dayOnly;
    return new Date(Number(y), Number(m) - 1, Number(d));
  }
  if (!/^\d{4}-\d{2}-\d{2}T/.test(dueAt)) return null;
  const parsed = new Date(dueAt);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** Monday 00:00 → next Monday 00:00 (local), containing `now`. */
export function getWeekRange(now: Date): { start: Date; end: Date } {
  const today = startOfDay(now);
  const mondayOffset = (today.getDay() + 6) % 7;
  const start = new Date(today);
  start.setDate(today.getDate() - mondayOffset);
  const end = new Date(start);
  end.setDate(start.getDate() + 7);
  return { start, end };
}

export type DueBucket = "this_week" | "later" | "no_date";

/** Overdue items count as "this week" so they stay in sight. */
export function getDueBucket(dueAt: string, now: Date): DueBucket {
  const due = parseDueDate(dueAt);
  if (!due) return "no_date";
  return due < getWeekRange(now).end ? "this_week" : "later";
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

function compareDue(a: TaskItem, b: TaskItem): number {
  const left = parseDueDate(a.dueAt)?.getTime() ?? Number.POSITIVE_INFINITY;
  const right = parseDueDate(b.dueAt)?.getTime() ?? Number.POSITIVE_INFINITY;
  return left - right;
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
  now: Date,
): TaskGroup[] {
  if (view === "done") {
    const done = tasks
      .filter((task) => task.done)
      .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
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
            .filter((task) => getDueBucket(task.dueAt, now) === bucket)
            .sort(compareDue),
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
      .map((group) => ({ ...group, tasks: group.tasks.sort(compareDue) }))
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
