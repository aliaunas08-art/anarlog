import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  DONE_TASK_LIMIT,
  OPEN_TASK_COUNT_SQL,
  TASKS_SQL,
  getDueBucket,
  getDueDayKey,
  getWeekRange,
  groupTasks,
  mapTaskRows,
  type TaskDateContext,
  type TaskItem,
  type TaskRow,
} from "./tasks-data";

// Sunday, Nov 15 2026 (Monday weeks run Mon Nov 9 – Sun Nov 15).
const NOW: TaskDateContext = {
  todayKey: "2026-11-15",
  weekStartsOn: 1,
  timezone: "UTC",
};

function task(overrides: Partial<TaskItem>): TaskItem {
  return {
    id: "t",
    text: "Task",
    status: "todo",
    done: false,
    dueAt: "",
    ownerId: "",
    ownerName: "",
    sessionId: "s1",
    sessionTitle: "Product sync",
    sessionDate: "2026-11-12T10:00:00.000Z",
    sourceOrder: 0,
    doneAt: "",
    ...overrides,
  };
}

describe("tasks-data", () => {
  it("maps rows and treats done/completed as done", () => {
    const [a, b, c] = mapTaskRows([
      {
        id: "a",
        status: "done",
        text: " Send deck ",
        due_at: "",
        assignee_human_id: "",
        owner_name: null,
        owner_email: null,
        session_id: "s1",
        session_title: "Sync",
        session_date: "",
        source_order: 0,
        done_at: "",
      },
      {
        id: "b",
        status: "completed",
        text: "x",
        due_at: null,
        assignee_human_id: "h1",
        owner_name: "",
        owner_email: "sam@example.com",
        session_id: null,
        session_title: null,
        session_date: null,
        source_order: null,
        done_at: null,
      },
      {
        id: "c",
        status: "in_progress",
        text: "y",
        due_at: "2026-11-20",
        assignee_human_id: "h2",
        owner_name: "Priya",
        owner_email: "",
        session_id: "s2",
        session_title: "",
        session_date: "",
        source_order: 2,
        done_at: "",
      },
    ]);
    expect(a).toMatchObject({ text: "Send deck", done: true, ownerName: "" });
    expect(b).toMatchObject({ done: true, ownerName: "sam@example.com" });
    expect(c).toMatchObject({
      done: false,
      ownerName: "Priya",
      dueAt: "2026-11-20",
    });
  });

  it("reads due days in the configured timezone", () => {
    expect(getDueDayKey("")).toBeNull();
    expect(getDueDayKey("End of week")).toBeNull();
    expect(getDueDayKey("2026-02-30")).toBeNull();
    expect(getDueDayKey("2026-11-20", "Asia/Tokyo")).toBe("2026-11-20");
    expect(getDueDayKey("2026-11-20T20:00:00Z", "UTC")).toBe("2026-11-20");
    expect(getDueDayKey("2026-11-20T20:00:00Z", "Asia/Tokyo")).toBe(
      "2026-11-21",
    );
  });

  it("buckets due dates by the configured week start", () => {
    expect(getWeekRange("2026-11-15", 1)).toEqual({
      start: "2026-11-09",
      end: "2026-11-16",
    });
    expect(getWeekRange("2026-11-15", 0)).toEqual({
      start: "2026-11-15",
      end: "2026-11-22",
    });
    expect(getDueBucket("2026-11-15", NOW)).toBe("this_week");
    expect(getDueBucket("2026-11-01", NOW)).toBe("this_week");
    expect(getDueBucket("2026-11-16", NOW)).toBe("later");
    expect(getDueBucket("2026-11-16", { ...NOW, weekStartsOn: 0 })).toBe(
      "this_week",
    );
    expect(getDueBucket("", NOW)).toBe("no_date");
  });

  it("groups open tasks for each view", () => {
    const tasks = [
      task({ id: "1", dueAt: "2026-11-14", ownerId: "h1", ownerName: "Sam" }),
      task({
        id: "2",
        dueAt: "2026-12-01",
        sessionId: "s2",
        sessionTitle: "Launch",
      }),
      task({ id: "3" }),
      task({ id: "4", done: true, doneAt: "2026-11-10" }),
      task({ id: "5", done: true, doneAt: "2026-11-11" }),
    ];

    const all = groupTasks(tasks, "all", NOW);
    expect(all.map((g) => g.bucket)).toEqual(["this_week", "later", "no_date"]);
    expect(all.flatMap((g) => g.tasks.map((t) => t.id))).toEqual([
      "1",
      "2",
      "3",
    ]);

    const person = groupTasks(tasks, "person", NOW);
    expect(person.map((g) => g.label)).toEqual(["Sam", ""]);

    const meeting = groupTasks(tasks, "meeting", NOW);
    expect(meeting.map((g) => g.sessionId)).toEqual(["s1", "s2"]);

    const done = groupTasks(tasks, "done", NOW);
    expect(done[0]?.tasks.map((t) => t.id)).toEqual(["5", "4"]);
  });
});

describe("tasks queries", () => {
  const { DatabaseSync } = createRequire(import.meta.url)(
    "node:sqlite",
  ) as typeof import("node:sqlite");
  let db: InstanceType<typeof DatabaseSync>;

  beforeEach(() => {
    db = new DatabaseSync(":memory:");
    db.exec(`
      CREATE TABLE sessions (
        id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '',
        started_at TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT '',
        deleted_at TEXT
      );
      CREATE TABLE session_documents (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL DEFAULT '',
        kind TEXT NOT NULL DEFAULT 'note', deleted_at TEXT
      );
      CREATE TABLE humans (
        id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '',
        email TEXT NOT NULL DEFAULT '', deleted_at TEXT
      );
      CREATE TABLE action_items (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL DEFAULT '',
        source_type TEXT NOT NULL DEFAULT '', source_id TEXT NOT NULL DEFAULT '',
        source_order INTEGER NOT NULL DEFAULT 0,
        assignee_human_id TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'todo', text TEXT NOT NULL DEFAULT '',
        due_at TEXT NOT NULL DEFAULT '', completed_at TEXT,
        updated_at TEXT NOT NULL DEFAULT '', deleted_at TEXT
      );
      INSERT INTO sessions (id, title, created_at, deleted_at) VALUES
        ('s1', 'Product sync', '2026-11-12', NULL),
        ('gone', 'Deleted meeting', '2026-11-10', '2026-11-11');
      INSERT INTO session_documents (id, session_id, kind, deleted_at) VALUES
        ('s1', 's1', 'note', NULL),
        ('summary-1', 's1', 'summary', NULL),
        ('summary-gone', 's1', 'summary', '2026-11-13');
    `);
  });
  afterEach(() => db.close());

  function insert(
    id: string,
    sourceType: string,
    sourceId: string,
    status = "todo",
    updatedAt = "2026-11-12",
  ) {
    db.prepare(
      `INSERT INTO action_items
         (id, session_id, source_type, source_id, status, text, updated_at)
       VALUES (?, '', ?, ?, ?, ?, ?)`,
    ).run(id, sourceType, sourceId, status, `Task ${id}`, updatedAt);
  }

  it("resolves the meeting of editor tasks and hides deleted sources", () => {
    insert("raw", "session_raw_note", "s1");
    insert("summary", "enhanced_note", "summary-1");
    insert("deleted-summary", "enhanced_note", "summary-gone");
    insert("deleted-meeting", "session_raw_note", "gone");
    insert("standalone", "document", "doc-1");

    const tasks = mapTaskRows(db.prepare(TASKS_SQL).all() as TaskRow[]);
    expect(
      Object.fromEntries(tasks.map((item) => [item.id, item.sessionTitle])),
    ).toEqual({ raw: "Product sync", summary: "Product sync", standalone: "" });
    expect(tasks.find((item) => item.id === "summary")?.sessionId).toBe("s1");

    const [count] = db.prepare(OPEN_TASK_COUNT_SQL).all() as Array<{
      count: number;
    }>;
    expect(count?.count).toBe(3);
  });

  it("loads every open task but only the most recent done tasks", () => {
    for (let index = 0; index < DONE_TASK_LIMIT + 5; index++) {
      insert(
        `done-${index}`,
        "session_raw_note",
        "s1",
        "done",
        `2026-11-12T00:00:00.${String(index).padStart(3, "0")}Z`,
      );
    }
    insert("open", "session_raw_note", "s1");

    const tasks = mapTaskRows(db.prepare(TASKS_SQL).all() as TaskRow[]);
    expect(tasks.filter((item) => item.done)).toHaveLength(DONE_TASK_LIMIT);
    expect(tasks.some((item) => item.id === "open")).toBe(true);
  });
});
