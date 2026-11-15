import { useMemo } from "react";

import type { JSONContent } from "@anlg/editor/note";
import type { TaskStorage } from "@anlg/editor/task-storage";
import {
  createTaskSourceKey,
  createTaskStatusAttrs,
  isSameTask,
  type TaskRecord,
  type TaskSource,
  type TaskStatus,
} from "@anlg/editor/tasks";

import { executeTransaction, liveQueryClient } from "~/db";
import { enqueueDatabaseWrite } from "~/db/write-queue";
import { useOwnerUserId } from "~/shared/owner-user";
import { DEFAULT_USER_ID } from "~/shared/utils";

export type SqliteTaskRow = {
  id: string;
  source_type: string;
  source_id: string;
  source_order: number;
  status: string;
  text: string;
  body_json: string;
  due_at: string;
  assignee_human_id?: string | null;
};

export type TaskStorageDependencies = {
  subscribe: typeof liveQueryClient.subscribe;
  execute: typeof liveQueryClient.execute;
  executeTransaction: typeof executeTransaction;
  enqueueWrite: (key: string, write: () => Promise<void>) => Promise<void>;
  now: () => string;
};

type SourceSubscription = {
  active: boolean;
  listeners: Set<() => void>;
  unsubscribePromise: Promise<() => Promise<void>>;
};

const emptyTasks: TaskRecord[] = [];
const resolvedOwnerSql = `
  COALESCE(
    NULLIF(?, ?),
    (
      SELECT NULLIF(json_extract(value_json, '$.account_user_id'), '')
      FROM app_settings
      WHERE id = 'cloudsync_workspace_binding'
    ),
    (
      SELECT NULLIF(json_extract(value_json, '$.workspace_id'), '')
      FROM app_settings
      WHERE id = 'cloudsync_workspace_binding'
    )
  )
`;

const defaultDependencies: TaskStorageDependencies = {
  subscribe: liveQueryClient.subscribe.bind(liveQueryClient),
  execute: liveQueryClient.execute.bind(liveQueryClient),
  executeTransaction,
  enqueueWrite: enqueueDatabaseWrite,
  now: () => new Date().toISOString(),
};

export function useStoreBackedTaskStorage(): TaskStorage {
  const ownerUserId = useOwnerUserId();

  return useMemo(
    () => createSqliteTaskStorage(ownerUserId || DEFAULT_USER_ID),
    [ownerUserId],
  );
}

export function createSqliteTaskStorage(
  ownerUserId = DEFAULT_USER_ID,
  dependencies: TaskStorageDependencies = defaultDependencies,
): TaskStorage {
  const sourceSnapshots = new Map<string, TaskRecord[]>();
  const taskSnapshots = new Map<string, TaskRecord>();
  const subscriptions = new Map<string, SourceSubscription>();

  const updateSourceSnapshot = (
    source: TaskSource,
    rows: SqliteTaskRow[],
  ): boolean => {
    const sourceKey = createTaskSourceKey(source);
    const previousTasks = sourceSnapshots.get(sourceKey) ?? emptyTasks;
    const nextTasks = rows
      .map(sqliteTaskRowToRecord)
      .filter((task): task is TaskRecord => task !== null);
    if (areSameTaskSets(previousTasks, nextTasks)) {
      return false;
    }

    sourceSnapshots.set(sourceKey, nextTasks);
    const nextTaskIds = new Set(nextTasks.map((task) => task.taskId));
    previousTasks.forEach((task) => {
      const cachedTask = taskSnapshots.get(task.taskId);
      if (
        !nextTaskIds.has(task.taskId) &&
        cachedTask?.sourceType === source.type &&
        cachedTask.sourceId === source.id
      ) {
        taskSnapshots.delete(task.taskId);
      }
    });
    nextTasks.forEach((task) => {
      const previousTask = taskSnapshots.get(task.taskId);
      taskSnapshots.set(
        task.taskId,
        previousTask && isSameTask(previousTask, task) ? previousTask : task,
      );
    });
    return true;
  };

  return {
    getTasksForSource(source) {
      return sourceSnapshots.get(createTaskSourceKey(source)) ?? emptyTasks;
    },
    subscribeSource(source, listener) {
      const sourceKey = createTaskSourceKey(source);
      let subscription = subscriptions.get(sourceKey);

      if (!subscription) {
        const listeners = new Set<() => void>();
        subscription = {
          active: true,
          listeners,
          unsubscribePromise: Promise.resolve(async () => {}),
        };
        const currentSubscription = subscription;
        currentSubscription.unsubscribePromise = dependencies
          .subscribe<SqliteTaskRow>(
            `
            SELECT
              id,
              source_type,
              source_id,
              source_order,
              status,
              text,
              body_json,
              due_at,
              assignee_human_id
            FROM action_items
            WHERE source_type = ? AND source_id = ? AND deleted_at IS NULL
            ORDER BY source_order, id
          `,
            [source.type, source.id],
            {
              onData: (rows) => {
                if (
                  currentSubscription.active &&
                  updateSourceSnapshot(source, rows)
                ) {
                  currentSubscription.listeners.forEach((notify) => notify());
                }
              },
              onError: (error) => {
                console.error(
                  `[tasks] failed to subscribe to ${sourceKey}`,
                  error,
                );
              },
            },
          )
          .catch((error) => {
            if (currentSubscription.active) {
              console.error(
                `[tasks] failed to start subscription for ${sourceKey}`,
                error,
              );
            }
            return async () => {};
          });
        subscriptions.set(sourceKey, currentSubscription);
      }

      subscription.listeners.add(listener);
      const currentSubscription = subscription;
      return () => {
        currentSubscription.listeners.delete(listener);
        if (currentSubscription.listeners.size > 0) {
          return;
        }

        currentSubscription.active = false;
        if (subscriptions.get(sourceKey) === currentSubscription) {
          subscriptions.delete(sourceKey);
          releaseSourceSnapshot(source);
        }
        void currentSubscription.unsubscribePromise
          .then((unsubscribe) => unsubscribe())
          .catch((error) => {
            console.error(
              `[tasks] failed to unsubscribe from ${sourceKey}`,
              error,
            );
          });
      };
    },
    getTask(taskId) {
      return taskSnapshots.get(taskId) ?? null;
    },
    upsertTasksForSource(source, tasks) {
      const currentTasks =
        sourceSnapshots.get(createTaskSourceKey(source)) ?? emptyTasks;
      if (areSameTaskSets(currentTasks, tasks)) {
        return;
      }

      const now = dependencies.now();
      const currentTasksById = new Map(
        currentTasks.map((currentTask) => [currentTask.taskId, currentTask]),
      );
      const retainedTaskIds = tasks.map((task) => task.taskId);
      const statements = [
        {
          sql: `
            UPDATE action_items
            SET deleted_at = ?, updated_at = ?, updated_by = ${resolvedOwnerSql}
            WHERE source_type = ?
              AND source_id = ?
              AND deleted_at IS NULL
              AND id NOT IN (SELECT value FROM json_each(?))
          `,
          params: [
            now,
            now,
            ownerUserId,
            DEFAULT_USER_ID,
            source.type,
            source.id,
            JSON.stringify(retainedTaskIds),
          ],
        },
        ...tasks.map((task) =>
          buildTaskUpsertStatement(
            task,
            ownerUserId,
            now,
            currentTasksById.get(task.taskId),
          ),
        ),
      ];

      persistTaskStatements(dependencies, statements);
    },
    removeTasksForSource(source, taskIds) {
      if (taskIds.length === 0) {
        return;
      }

      const now = dependencies.now();
      persistTaskStatements(dependencies, [
        {
          sql: `
            UPDATE action_items
            SET deleted_at = ?, updated_at = ?, updated_by = ${resolvedOwnerSql}
            WHERE source_type = ?
              AND source_id = ?
              AND deleted_at IS NULL
              AND id IN (SELECT value FROM json_each(?))
          `,
          params: [
            now,
            now,
            ownerUserId,
            DEFAULT_USER_ID,
            source.type,
            source.id,
            JSON.stringify(taskIds),
          ],
        },
      ]);
    },
    moveTasksToSource(taskIds, nextSource, insertionOrder) {
      if (taskIds.length === 0) {
        return;
      }

      const now = dependencies.now();
      persistTaskStatements(
        dependencies,
        taskIds.map((taskId, index) => ({
          sql: `
            UPDATE action_items
            SET
              session_id = ${taskSessionIdSql(nextSource.type)},
              source_type = ?,
              source_id = ?,
              source_order = ?,
              updated_at = ?,
              updated_by = ${resolvedOwnerSql}
            WHERE id = ? AND deleted_at IS NULL
          `,
          params: [
            taskSessionIdParam(nextSource),
            nextSource.type,
            nextSource.id,
            insertionOrder + index,
            now,
            ownerUserId,
            DEFAULT_USER_ID,
            taskId,
          ],
        })),
      );
    },
  };

  function releaseSourceSnapshot(source: TaskSource) {
    const sourceKey = createTaskSourceKey(source);
    const tasks = sourceSnapshots.get(sourceKey);
    sourceSnapshots.delete(sourceKey);
    for (const task of tasks ?? []) {
      const cachedTask = taskSnapshots.get(task.taskId);
      if (
        cachedTask?.sourceType === source.type &&
        cachedTask.sourceId === source.id
      ) {
        taskSnapshots.delete(task.taskId);
      }
    }
  }
}

function persistTaskStatements(
  dependencies: TaskStorageDependencies,
  statements: Array<{ sql: string; params: unknown[] }>,
) {
  void dependencies
    .enqueueWrite("tasks", async () => {
      await dependencies.executeTransaction(statements);
    })
    .catch((error) => {
      console.error("[tasks] failed to persist task changes", error);
    });
}

/**
 * Owner and due date are only sent when the editor changed them against the
 * snapshot it read them from; otherwise they go out empty so the upsert keeps
 * the stored values. Echoing the snapshot could undo a Tasks-view change that
 * landed after the snapshot (e.g. inside the editor's save debounce).
 */
function changedTaskField(
  next: string | undefined,
  previous: string | undefined,
  hasPrevious: boolean,
): string {
  if (hasPrevious && (next ?? "") === (previous ?? "")) return "";
  return next ?? "";
}

function buildTaskUpsertStatement(
  task: TaskRecord,
  ownerUserId: string,
  now: string,
  previousTask?: TaskRecord,
) {
  const sessionIdSql = taskSessionIdSql(task.sourceType);
  const sessionIdParam = taskSessionIdParam({
    type: task.sourceType,
    id: task.sourceId,
  });
  return {
    sql: `
      INSERT INTO action_items (
        id,
        workspace_id,
        session_id,
        source_type,
        source_id,
        source_order,
        assignee_human_id,
        status,
        text,
        body_json,
        due_at,
        completed_at,
        created_by,
        updated_by,
        metadata_json,
        created_at,
        updated_at,
        deleted_at
      )
      VALUES (
        ?, COALESCE(
          (
            SELECT NULLIF(workspace_id, '')
            FROM sessions
            WHERE id = ${sessionIdSql} AND deleted_at IS NULL
          ),
          (
            SELECT NULLIF(json_extract(value_json, '$.workspace_id'), '')
            FROM app_settings
            WHERE id = 'cloudsync_workspace_binding'
          )
        ), ${sessionIdSql}, ?, ?, ?, ?, ?, ?, ?, ?, ?, ${resolvedOwnerSql}, ${resolvedOwnerSql}, '{}', ?, ?, NULL
      )
      ON CONFLICT(id) DO UPDATE SET
        session_id = excluded.session_id,
        source_type = excluded.source_type,
        source_id = excluded.source_id,
        source_order = excluded.source_order,
        status = excluded.status,
        text = excluded.text,
        body_json = excluded.body_json,
        -- Empty means "the editor didn't change it" (see changedTaskField),
        -- so what was set elsewhere (e.g. the Tasks view) is kept.
        assignee_human_id = CASE
          WHEN excluded.assignee_human_id <> ''
            THEN excluded.assignee_human_id
          ELSE action_items.assignee_human_id
        END,
        due_at = CASE
          WHEN excluded.due_at <> '' THEN excluded.due_at
          ELSE action_items.due_at
        END,
        completed_at = CASE
          WHEN excluded.status = 'done'
            THEN COALESCE(action_items.completed_at, excluded.completed_at)
          ELSE NULL
        END,
        updated_by = excluded.updated_by,
        updated_at = excluded.updated_at,
        deleted_at = NULL
    `,
    params: [
      task.taskId,
      sessionIdParam,
      sessionIdParam,
      task.sourceType,
      task.sourceId,
      task.sourceOrder,
      changedTaskField(
        task.assigneeHumanId,
        previousTask?.assigneeHumanId,
        previousTask !== undefined,
      ),
      task.status,
      task.textPreview,
      JSON.stringify(task.body),
      changedTaskField(
        task.dueDate,
        previousTask?.dueDate,
        previousTask !== undefined,
      ),
      task.status === "done" ? now : null,
      ownerUserId,
      DEFAULT_USER_ID,
      ownerUserId,
      DEFAULT_USER_ID,
      now,
      now,
    ],
  };
}

function sqliteTaskRowToRecord(row: SqliteTaskRow): TaskRecord | null {
  const sourceOrder = Number(row.source_order);
  if (
    !row.id ||
    !row.source_id ||
    !row.source_type ||
    !Number.isFinite(sourceOrder) ||
    (row.status !== "todo" &&
      row.status !== "in_progress" &&
      row.status !== "done")
  ) {
    return null;
  }

  const body = parseTaskBody(row.body_json, row.text);
  return {
    taskId: row.id,
    sourceId: row.source_id,
    sourceType: row.source_type,
    sourceOrder,
    status: row.status,
    textPreview: row.text || getTextPreview(body),
    body,
    dueDate: row.due_at || undefined,
    assigneeHumanId: row.assignee_human_id || undefined,
  };
}

function areSameTaskSets(left: TaskRecord[], right: TaskRecord[]) {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((task, index) => isSameTask(task, right[index]!));
}

function parseTaskBody(bodyJson: unknown, legacyText: unknown): JSONContent[] {
  if (typeof bodyJson === "string" && bodyJson) {
    try {
      const parsed = JSON.parse(bodyJson);
      if (Array.isArray(parsed)) {
        return parsed as JSONContent[];
      }
    } catch {}
  }

  if (typeof legacyText === "string" && legacyText) {
    return [
      {
        type: "paragraph",
        content: [{ type: "text", text: legacyText }],
      },
    ];
  }

  return [{ type: "paragraph" }];
}

function getTextPreview(body: JSONContent[]): string {
  const firstParagraph = body.find((node) => node.type === "paragraph");
  return getNodeText(firstParagraph).trim();
}

function getNodeText(node: JSONContent | undefined): string {
  if (!node) {
    return "";
  }

  if (typeof node.text === "string") {
    return node.text;
  }

  return (node.content ?? []).map((child) => getNodeText(child)).join(" ");
}

// Editor sources are `session_raw_note` (id = session id) and `enhanced_note`
// (id = session_documents id); `session` is the legacy source type.
const SESSION_ID_SOURCE_TYPES = new Set(["session", "session_raw_note"]);

function taskSessionIdSql(sourceType: string) {
  return sourceType === "enhanced_note"
    ? `COALESCE((SELECT NULLIF(session_id, '') FROM session_documents WHERE id = ?), '')`
    : "?";
}

function taskSessionIdParam(source: TaskSource) {
  return source.type === "enhanced_note" ||
    SESSION_ID_SOURCE_TYPES.has(source.type)
    ? source.id
    : "";
}

/**
 * The meeting an `action_items` row belongs to. Rows written before
 * `session_id` was filled for editor sources still resolve through their
 * source.
 */
export function resolvedTaskSessionIdSql(alias: string) {
  return `
    COALESCE(
      NULLIF(${alias}.session_id, ''),
      CASE ${alias}.source_type
        WHEN 'session_raw_note' THEN ${alias}.source_id
        WHEN 'session' THEN ${alias}.source_id
        WHEN 'enhanced_note' THEN (
          SELECT NULLIF(document.session_id, '')
          FROM session_documents AS document
          WHERE document.id = ${alias}.source_id
        )
      END,
      ''
    )
  `;
}

type ActionItemSourceRow = {
  source_type: string;
  source_id: string;
  session_id: string;
};

/**
 * Sets the status of a single action item (e.g. from the Tasks view) and the
 * matching `taskItem` in its source note, so the editor doesn't revert the
 * change from the note's stored copy the next time it syncs. The note is
 * written through its session's write queue (like note saves) and in the same
 * transaction as the action item.
 */
export async function setActionItemStatus(
  taskId: string,
  status: TaskStatus,
  ownerUserId = DEFAULT_USER_ID,
  dependencies: TaskStorageDependencies = defaultDependencies,
): Promise<void> {
  try {
    const [item] = await dependencies.execute<ActionItemSourceRow>(
      `
        SELECT
          source_type,
          source_id,
          ${resolvedTaskSessionIdSql("item")} AS session_id
        FROM action_items AS item
        WHERE id = ? AND deleted_at IS NULL
      `,
      [taskId],
    );
    if (!item) return;

    const write = async () => {
      const now = dependencies.now();
      const statements: Array<{ sql: string; params: unknown[] }> = [
        {
          sql: `
            UPDATE action_items
            SET
              status = ?,
              completed_at = CASE
                WHEN ? = 'done' THEN COALESCE(completed_at, ?)
                ELSE NULL
              END,
              updated_at = ?,
              updated_by = ${resolvedOwnerSql}
            WHERE id = ? AND deleted_at IS NULL
          `,
          params: [
            status,
            status,
            now,
            now,
            ownerUserId || DEFAULT_USER_ID,
            DEFAULT_USER_ID,
            taskId,
          ],
        },
      ];

      const documentId = getTaskDocumentId(item);
      if (documentId) {
        const [document] = await dependencies.execute<{
          body: string;
          body_format: string;
        }>(
          `
            SELECT body, body_format
            FROM session_documents
            WHERE id = ? AND deleted_at IS NULL
          `,
          [documentId],
        );
        const nextBody =
          document?.body_format === "prosemirror_json"
            ? setTaskStatusInDocumentBody(document.body, taskId, status)
            : null;
        if (document && nextBody !== null) {
          statements.push({
            sql: `
              UPDATE session_documents
              SET body = ?, updated_at = ?
              WHERE id = ? AND body = ? AND deleted_at IS NULL
            `,
            params: [nextBody, now, documentId, document.body],
          });
        }
      }

      await dependencies.executeTransaction(statements);
    };

    await dependencies.enqueueWrite(
      item.session_id ? `session:${item.session_id}` : "tasks",
      item.session_id ? () => dependencies.enqueueWrite("tasks", write) : write,
    );
  } catch (error) {
    console.error("[tasks] failed to persist task status", error);
  }
}

function getTaskDocumentId(item: ActionItemSourceRow): string | null {
  if (item.source_type === "session_raw_note") return item.source_id;
  if (item.source_type === "enhanced_note") return item.source_id;
  return null;
}

/**
 * Returns the serialized document with the task's `taskItem` set to `status`,
 * or null when the body isn't readable or has nothing to change.
 */
export function setTaskStatusInDocumentBody(
  body: string,
  taskId: string,
  status: TaskStatus,
): string | null {
  let parsed: JSONContent;
  try {
    parsed = JSON.parse(body) as JSONContent;
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object") return null;

  const attrs = createTaskStatusAttrs(status);
  let changed = false;
  const visit = (node: JSONContent) => {
    if (node.type === "taskItem" && node.attrs?.taskId === taskId) {
      if (
        node.attrs.status !== attrs.status ||
        node.attrs.checked !== attrs.checked
      ) {
        node.attrs = { ...node.attrs, ...attrs };
        changed = true;
      }
    }
    node.content?.forEach(visit);
  };
  visit(parsed);

  return changed ? JSON.stringify(parsed) : null;
}

const DAY_KEY_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Sets (or clears, with "") the owner of a single action item, e.g. from the
 * Tasks view. `humanId` is a `humans.id`. The note isn't touched: the editor
 * keeps stored owners unless the task mentions a person.
 */
export function setActionItemOwner(
  taskId: string,
  humanId: string,
  ownerUserId = DEFAULT_USER_ID,
  dependencies: TaskStorageDependencies = defaultDependencies,
): Promise<void> {
  return updateActionItemField(
    taskId,
    "assignee_human_id",
    humanId,
    ownerUserId,
    dependencies,
  );
}

/**
 * Sets the due date of a single action item as a `YYYY-MM-DD` day, or clears
 * it with "". Anything else is ignored.
 */
export function setActionItemDue(
  taskId: string,
  dueDay: string,
  ownerUserId = DEFAULT_USER_ID,
  dependencies: TaskStorageDependencies = defaultDependencies,
): Promise<void> {
  if (dueDay && !DAY_KEY_PATTERN.test(dueDay)) {
    console.error(`[tasks] ignoring invalid due date "${dueDay}"`);
    return Promise.resolve();
  }
  return updateActionItemField(
    taskId,
    "due_at",
    dueDay,
    ownerUserId,
    dependencies,
  );
}

async function updateActionItemField(
  taskId: string,
  column: "assignee_human_id" | "due_at",
  value: string,
  ownerUserId: string,
  dependencies: TaskStorageDependencies,
): Promise<void> {
  try {
    await dependencies.enqueueWrite("tasks", async () => {
      const now = dependencies.now();
      await dependencies.executeTransaction([
        {
          sql: `
            UPDATE action_items
            SET
              ${column} = ?,
              updated_at = ?,
              updated_by = ${resolvedOwnerSql}
            WHERE id = ? AND deleted_at IS NULL
          `,
          params: [
            value,
            now,
            ownerUserId || DEFAULT_USER_ID,
            DEFAULT_USER_ID,
            taskId,
          ],
        },
      ]);
    });
  } catch (error) {
    console.error(`[tasks] failed to persist task ${column}`, error);
  }
}
