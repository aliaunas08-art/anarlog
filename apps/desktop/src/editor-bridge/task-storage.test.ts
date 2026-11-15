import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TaskRecord, TaskSource } from "@anlg/editor/tasks";

import {
  createSqliteTaskStorage,
  setActionItemStatus,
  type SqliteTaskRow,
  type TaskStorageDependencies,
} from "./task-storage";

import { DEFAULT_USER_ID } from "~/shared/utils";

const sessionSource: TaskSource = {
  type: "session_raw_note",
  id: "session-1",
};
const task: TaskRecord = {
  taskId: "task-1",
  sourceId: "session-1",
  sourceType: "session_raw_note",
  sourceOrder: 0,
  status: "todo",
  textPreview: "Follow up",
  body: [
    {
      type: "paragraph",
      content: [{ type: "text", text: "Follow up" }],
    },
  ],
  dueDate: "2026-07-12",
};

function sqliteTaskRow(overrides: Partial<SqliteTaskRow> = {}): SqliteTaskRow {
  return {
    id: "task-1",
    source_type: "session_raw_note",
    source_id: "session-1",
    source_order: 0,
    status: "todo",
    text: "Follow up",
    body_json:
      '[{"type":"paragraph","content":[{"type":"text","text":"Follow up"}]}]',
    due_at: "2026-07-12",
    ...overrides,
  };
}

function createHarness() {
  const subscriptions: Array<{
    onData: (rows: SqliteTaskRow[]) => void;
    onError?: (error: string) => void;
  }> = [];
  const unsubscribe = vi.fn().mockResolvedValue(undefined);
  const executeTransaction = vi.fn().mockResolvedValue([1]);
  const enqueueWrite = vi.fn(
    async (_key: string, write: () => Promise<void>) => {
      await write();
    },
  );
  const subscribe = (async (
    _sql: string,
    _params: unknown[],
    options: {
      onData: (rows: SqliteTaskRow[]) => void;
      onError?: (error: string) => void;
    },
  ) => {
    subscriptions.push(options);
    return unsubscribe;
  }) as TaskStorageDependencies["subscribe"];
  const dependencies: TaskStorageDependencies = {
    subscribe,
    execute: vi.fn().mockResolvedValue([]),
    executeTransaction,
    enqueueWrite,
    now: () => "2026-07-10T10:00:00.000Z",
  };

  return {
    dependencies,
    enqueueWrite,
    executeTransaction,
    subscriptions,
    unsubscribe,
  };
}

describe("SQLite task storage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("publishes stable source snapshots from one SQLite subscription", async () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);
    const listener = vi.fn();

    const unsubscribe = storage.subscribeSource(sessionSource, listener);
    expect(harness.subscriptions).toHaveLength(1);
    expect(storage.getTasksForSource(sessionSource)).toEqual([]);

    harness.subscriptions[0].onData([sqliteTaskRow()]);
    const firstSnapshot = storage.getTasksForSource(sessionSource);
    expect(firstSnapshot).toEqual([task]);
    expect(storage.getTask("task-1")).toBe(firstSnapshot[0]);
    expect(listener).toHaveBeenCalledOnce();

    harness.subscriptions[0].onData([sqliteTaskRow()]);
    expect(storage.getTasksForSource(sessionSource)).toBe(firstSnapshot);
    expect(listener).toHaveBeenCalledOnce();

    harness.subscriptions[0].onData([
      sqliteTaskRow({ text: "Updated follow up" }),
    ]);
    expect(listener).toHaveBeenCalledTimes(2);
    expect(storage.getTask("task-1")?.textPreview).toBe("Updated follow up");

    unsubscribe();
    await vi.waitFor(() => expect(harness.unsubscribe).toHaveBeenCalledOnce());
    expect(storage.getTasksForSource(sessionSource)).toEqual([]);
    expect(storage.getTask("task-1")).toBeNull();
  });

  it("retains a shared task snapshot until its final source unsubscribes", () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);
    const firstUnsubscribe = storage.subscribeSource(sessionSource, vi.fn());
    const secondUnsubscribe = storage.subscribeSource(sessionSource, vi.fn());
    harness.subscriptions[0].onData([sqliteTaskRow()]);

    firstUnsubscribe();
    expect(storage.getTasksForSource(sessionSource)).toEqual([task]);
    expect(storage.getTask("task-1")).toEqual(task);

    secondUnsubscribe();
    expect(storage.getTasksForSource(sessionSource)).toEqual([]);
    expect(storage.getTask("task-1")).toBeNull();
  });

  it("atomically tombstones removed rows and upserts the source snapshot", async () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);

    storage.upsertTasksForSource(sessionSource, [task]);

    await vi.waitFor(() =>
      expect(harness.executeTransaction).toHaveBeenCalledOnce(),
    );
    expect(harness.enqueueWrite).toHaveBeenCalledWith(
      "tasks",
      expect.any(Function),
    );
    const statements = harness.executeTransaction.mock.calls[0][0];
    expect(statements).toHaveLength(2);
    expect(statements[0].sql).toContain("SET deleted_at = ?");
    expect(statements[0].sql).toContain("$.account_user_id");
    expect(statements[0].sql).toContain("json_each(?)");
    expect(statements[1].sql).toContain("INSERT INTO action_items");
    expect(statements[1].sql).toContain("SELECT NULLIF(workspace_id");
    expect(statements[1].sql).toContain("cloudsync_workspace_binding");
    expect(statements[1].sql).toContain("$.account_user_id");
    expect(statements[1].sql).toContain("NULLIF(workspace_id, '')");
    expect(statements[1].sql).not.toContain("\n          ''\n");
    expect(statements[1].sql).toContain("deleted_at = NULL");
    expect(statements[1].params).toContain("task-1");
    expect(statements[1].params).toContain("Follow up");
  });

  it("resolves legacy default task ownership from the workspace binding", async () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage(
      DEFAULT_USER_ID,
      harness.dependencies,
    );

    storage.upsertTasksForSource(sessionSource, [task]);

    await vi.waitFor(() =>
      expect(harness.executeTransaction).toHaveBeenCalledOnce(),
    );
    const statements = harness.executeTransaction.mock.calls[0][0];
    expect(statements[0].sql).toContain("$.account_user_id");
    expect(statements[1].sql).toContain("$.account_user_id");
    expect(
      statements.flatMap(
        (statement: { params: unknown[] }) => statement.params,
      ),
    ).toContain(DEFAULT_USER_ID);
  });

  it("skips writes when the committed source snapshot is unchanged", () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);
    storage.subscribeSource(sessionSource, vi.fn());
    harness.subscriptions[0].onData([sqliteTaskRow()]);

    storage.upsertTasksForSource(sessionSource, [task]);

    expect(harness.enqueueWrite).not.toHaveBeenCalled();
    expect(harness.executeTransaction).not.toHaveBeenCalled();
  });

  it("scopes removals to the source and moves tasks in one batch", async () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);

    storage.removeTasksForSource(sessionSource, ["task-1"]);
    storage.moveTasksToSource(
      ["task-1", "task-2"],
      { type: "document", id: "document-1" },
      4,
    );

    await vi.waitFor(() =>
      expect(harness.executeTransaction).toHaveBeenCalledTimes(2),
    );
    const removal = harness.executeTransaction.mock.calls[0][0][0];
    expect(removal.sql).toContain("source_type = ?");
    expect(removal.sql).toContain("id IN (SELECT value FROM json_each(?))");
    expect(removal.params.slice(-3)).toEqual([
      "session_raw_note",
      "session-1",
      '["task-1"]',
    ]);

    const moves = harness.executeTransaction.mock.calls[1][0];
    expect(moves).toHaveLength(2);
    expect(moves[0].params).toEqual([
      "",
      "document",
      "document-1",
      4,
      "2026-07-10T10:00:00.000Z",
      "user-1",
      DEFAULT_USER_ID,
      "task-1",
    ]);
    expect(moves[1].params[3]).toBe(5);
  });

  it("drops malformed rows instead of exposing invalid task records", () => {
    const harness = createHarness();
    const storage = createSqliteTaskStorage("user-1", harness.dependencies);
    const listener = vi.fn();
    storage.subscribeSource(sessionSource, listener);

    harness.subscriptions[0].onData([
      sqliteTaskRow({ id: "", status: "unknown" }),
    ]);

    expect(storage.getTasksForSource(sessionSource)).toEqual([]);
    expect(storage.getTask("task-1")).toBeNull();
    expect(listener).not.toHaveBeenCalled();
  });

});

describe("SQLite task storage persistence", () => {
  const { DatabaseSync } = createRequire(import.meta.url)(
    "node:sqlite",
  ) as typeof import("node:sqlite");
  let db: InstanceType<typeof DatabaseSync>;
  let dependencies: TaskStorageDependencies;

  const noteBody = (status: string, checked: boolean) =>
    JSON.stringify({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { status, checked, taskId: "task-1", taskItemId: "i1" },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "Follow up" }],
                },
              ],
            },
          ],
        },
      ],
    });

  beforeEach(() => {
    db = new DatabaseSync(":memory:");
    db.exec(`
      CREATE TABLE app_settings (id TEXT PRIMARY KEY, value_json TEXT NOT NULL);
      CREATE TABLE sessions (
        id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT '', deleted_at TEXT
      );
      CREATE TABLE session_documents (
        id TEXT PRIMARY KEY, session_id TEXT NOT NULL DEFAULT '',
        body TEXT NOT NULL DEFAULT '',
        body_format TEXT NOT NULL DEFAULT 'prosemirror_json',
        updated_at TEXT NOT NULL DEFAULT '', deleted_at TEXT
      );
      CREATE TABLE action_items (
        id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL DEFAULT '',
        session_id TEXT NOT NULL DEFAULT '', source_type TEXT NOT NULL DEFAULT '',
        source_id TEXT NOT NULL DEFAULT '', source_order INTEGER NOT NULL DEFAULT 0,
        assignee_human_id TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'todo', text TEXT NOT NULL DEFAULT '',
        body_json TEXT NOT NULL DEFAULT '{}', due_at TEXT NOT NULL DEFAULT '',
        completed_at TEXT, created_by TEXT NOT NULL DEFAULT '',
        updated_by TEXT NOT NULL DEFAULT '',
        metadata_json TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL DEFAULT '',
        deleted_at TEXT
      );
      INSERT INTO sessions (id, workspace_id) VALUES ('session-1', 'ws-1');
    `);
    db.prepare(
      "INSERT INTO session_documents (id, session_id, body) VALUES (?, ?, ?)",
    ).run("session-1", "session-1", noteBody("todo", false));
    db.prepare(
      "INSERT INTO session_documents (id, session_id, body) VALUES (?, ?, ?)",
    ).run("summary-1", "session-1", noteBody("todo", false));
    dependencies = {
      subscribe: vi.fn() as unknown as TaskStorageDependencies["subscribe"],
      execute: (async (sql: string, params: unknown[] = []) =>
        db
          .prepare(sql)
          .all(
            ...(params as Array<string | number | null>),
          )) as TaskStorageDependencies["execute"],
      executeTransaction: (async (
        statements: Array<{ sql: string; params: unknown[] }>,
      ) => {
        db.exec("BEGIN");
        for (const statement of statements) {
          db.prepare(statement.sql).run(
            ...(statement.params as Array<string | number | null>),
          );
        }
        db.exec("COMMIT");
        return [];
      }) as unknown as TaskStorageDependencies["executeTransaction"],
      enqueueWrite: (_key, write) => write(),
      now: () => "2026-07-10T10:00:00.000Z",
    };
  });
  afterEach(() => db.close());

  const actionItem = () =>
    db
      .prepare(
        "SELECT session_id, workspace_id, status, completed_at FROM action_items WHERE id = 'task-1'",
      )
      .get() as {
      session_id: string;
      workspace_id: string;
      status: string;
      completed_at: string | null;
    };
  const documentTaskAttrs = (id: string) => {
    const { body } = db
      .prepare("SELECT body FROM session_documents WHERE id = ?")
      .get(id) as { body: string };
    return JSON.parse(body).content[0].content[0].attrs;
  };

  it("stores the meeting of raw-note and summary tasks", async () => {
    const storage = createSqliteTaskStorage("user-1", dependencies);
    storage.upsertTasksForSource({ type: "enhanced_note", id: "summary-1" }, [
      { ...task, sourceType: "enhanced_note", sourceId: "summary-1" },
    ]);
    await vi.waitFor(() =>
      expect(actionItem()).toMatchObject({
        session_id: "session-1",
        workspace_id: "ws-1",
      }),
    );

    storage.upsertTasksForSource(sessionSource, [task]);
    await vi.waitFor(() =>
      expect(actionItem()).toMatchObject({ session_id: "session-1" }),
    );
  });

  it("checks a task off in its note so the editor keeps the new status", async () => {
    createSqliteTaskStorage("user-1", dependencies).upsertTasksForSource(
      sessionSource,
      [task],
    );
    await vi.waitFor(() => expect(actionItem()).toBeTruthy());

    await setActionItemStatus("task-1", "done", "user-1", dependencies);
    expect(actionItem()).toMatchObject({
      status: "done",
      completed_at: "2026-07-10T10:00:00.000Z",
    });
    expect(documentTaskAttrs("session-1")).toMatchObject({
      status: "done",
      checked: true,
      taskId: "task-1",
    });
    expect(documentTaskAttrs("summary-1")).toMatchObject({ status: "todo" });

    await setActionItemStatus("task-1", "in_progress", "user-1", dependencies);
    expect(actionItem()).toMatchObject({
      status: "in_progress",
      completed_at: null,
    });
    expect(documentTaskAttrs("session-1")).toMatchObject({
      status: "in_progress",
      checked: false,
    });
  });
});
