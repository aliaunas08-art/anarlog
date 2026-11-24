import { describe, expect, it } from "vitest";

import { createInMemoryTaskStorage } from "./task-storage";
import {
  createTaskItemNode,
  extractTasksFromContent,
  getNextTaskStatus,
  hydrateTaskContent,
  normalizeTaskContent,
} from "./tasks";

describe("task content", () => {
  it("toggles task status through the simple checked state", () => {
    expect(getNextTaskStatus("todo")).toBe("done");
    expect(getNextTaskStatus("in_progress")).toBe("done");
    expect(getNextTaskStatus("done")).toBe("todo");
  });

  it("assigns unique task ids and task item ids to missing and duplicated task items", () => {
    const content = normalizeTaskContent({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { checked: false, taskId: "duplicate-task" },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "First task" }],
                },
              ],
            },
            {
              type: "taskItem",
              attrs: { checked: true, taskId: "duplicate-task" },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "Second task" }],
                },
              ],
            },
            {
              type: "taskItem",
              attrs: { checked: false },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "Third task" }],
                },
              ],
            },
          ],
        },
      ],
    });

    const taskItems =
      content?.content?.[0]?.content?.filter(
        (node) => node.type === "taskItem",
      ) ?? [];
    const taskIds = taskItems.map((node) => node.attrs?.taskId);
    const taskItemIds = taskItems.map((node) => node.attrs?.taskItemId);

    expect(taskIds).toHaveLength(3);
    expect(taskIds[0]).toBe("duplicate-task");
    expect(taskIds[1]).toEqual(expect.any(String));
    expect(taskIds[2]).toEqual(expect.any(String));
    expect(new Set(taskIds).size).toBe(3);
    expect(taskItemIds).toHaveLength(3);
    expect(taskItemIds[0]).toEqual(expect.any(String));
    expect(taskItemIds[1]).toEqual(expect.any(String));
    expect(taskItemIds[2]).toEqual(expect.any(String));
    expect(new Set(taskItemIds).size).toBe(3);
  });

  it("extracts canonical task rows with full body content", () => {
    const source = { type: "daily_note", id: "2026-04-06" };
    const content = normalizeTaskContent({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { checked: false, taskId: "task-1" },
              content: [
                {
                  type: "paragraph",
                  content: [
                    { type: "text", text: "Parent task" },
                    { type: "hardBreak" },
                    {
                      type: "text",
                      marks: [{ type: "bold" }],
                      text: "still here",
                    },
                  ],
                },
                {
                  type: "blockquote",
                  content: [
                    {
                      type: "paragraph",
                      content: [{ type: "text", text: "Nested detail" }],
                    },
                  ],
                },
              ],
            },
          ],
        },
      ],
    });

    const tasks = extractTasksFromContent(content!, source);

    expect(tasks).toHaveLength(1);
    expect(tasks[0]).toMatchObject({
      taskId: "task-1",
      sourceId: source.id,
      sourceType: source.type,
      sourceOrder: 0,
      status: "todo",
      textPreview: "Parent task still here",
    });
    expect(createTaskItemNode(tasks[0]!)).toMatchObject({
      type: "taskItem",
      attrs: {
        checked: false,
        taskId: "task-1",
        taskItemId: expect.any(String),
      },
      content: tasks[0]?.body,
    });
  });

  it("preserves in-progress status when extracting and rebuilding tasks", () => {
    const source = { type: "daily_note", id: "2026-04-06" };
    const content = normalizeTaskContent({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: {
                status: "in_progress",
                checked: false,
                taskId: "task-2",
              },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "Working on it" }],
                },
              ],
            },
          ],
        },
      ],
    });

    const [task] = extractTasksFromContent(content!, source);

    expect(task).toMatchObject({
      taskId: "task-2",
      status: "in_progress",
      textPreview: "Working on it",
    });
    expect(createTaskItemNode(task!)).toMatchObject({
      type: "taskItem",
      attrs: {
        status: "in_progress",
        checked: false,
        taskId: "task-2",
        taskItemId: expect.any(String),
      },
    });
  });

  it("hydrates a source from canonical tasks and removes moved-away tasks", () => {
    const source = { type: "daily_note", id: "2026-04-06" };
    const foreignTask = {
      taskId: "task-foreign",
      sourceId: "2026-04-05",
      sourceType: "daily_note",
      sourceOrder: 0,
      status: "todo" as const,
      textPreview: "Foreign task",
      body: [
        {
          type: "paragraph",
          content: [{ type: "text", text: "Foreign task" }],
        },
      ],
    };
    const sourceTask = {
      taskId: "task-1",
      sourceId: source.id,
      sourceType: source.type,
      sourceOrder: 0,
      status: "done" as const,
      textPreview: "Hydrated task",
      dueDate: "2026-04-12",
      body: [
        {
          type: "paragraph",
          content: [{ type: "text", text: "Hydrated task" }],
        },
      ],
    };
    const content = normalizeTaskContent({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { checked: false, taskId: "task-1" },
              content: [{ type: "paragraph" }],
            },
            {
              type: "taskItem",
              attrs: { checked: false, taskId: "task-foreign" },
              content: [
                {
                  type: "paragraph",
                  content: [{ type: "text", text: "Stale copy" }],
                },
              ],
            },
          ],
        },
      ],
    });

    const hydrated = hydrateTaskContent({
      content: content!,
      sourceTasks: [sourceTask],
      getTask: (taskId) =>
        taskId === foreignTask.taskId ? foreignTask : sourceTask,
    });

    expect(hydrated).toMatchObject({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { checked: true, taskId: "task-1" },
            },
          ],
        },
      ],
    });
  });
});

describe("task owners from mentions", () => {
  const mention = (id: string, type = "human") => ({
    type: "mention-@",
    attrs: { id, type, label: id },
  });
  const taskItem = (taskId: string, content: unknown[]) => ({
    type: "taskItem",
    attrs: { status: "todo", taskId, taskItemId: `item-${taskId}` },
    content,
  });
  const source = { type: "session_raw_note", id: "session-1" };

  it("uses the first person mentioned in the task, not nested tasks", () => {
    const content = {
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            taskItem("task-1", [
              {
                type: "paragraph",
                content: [
                  mention("org-1", "organization"),
                  { type: "text", text: " ask " },
                  mention("human-1"),
                  mention("human-2"),
                ],
              },
              {
                type: "taskList",
                content: [
                  taskItem("task-2", [
                    { type: "paragraph", content: [mention("human-3")] },
                  ]),
                ],
              },
            ]),
            taskItem("task-3", [
              { type: "paragraph", content: [{ type: "text", text: "x" }] },
            ]),
          ],
        },
      ],
    };

    const tasks = extractTasksFromContent(
      content as never,
      source,
      new Map([
        [
          "task-1",
          {
            taskId: "task-1",
            sourceId: "session-1",
            sourceType: "session_raw_note",
            sourceOrder: 0,
            status: "todo" as const,
            textPreview: "",
            body: [],
            assigneeHumanId: "human-9",
          },
        ],
        [
          "task-3",
          {
            taskId: "task-3",
            sourceId: "session-1",
            sourceType: "session_raw_note",
            sourceOrder: 2,
            status: "todo" as const,
            textPreview: "x",
            body: [],
            assigneeHumanId: "human-9",
            dueDate: "2026-05-01",
          },
        ],
      ]),
    );

    expect(
      tasks.map(({ taskId, assigneeHumanId, dueDate }) => ({
        taskId,
        assigneeHumanId,
        dueDate,
      })),
    ).toEqual([
      // The mention wins over the stored owner.
      { taskId: "task-1", assigneeHumanId: "human-1", dueDate: undefined },
      { taskId: "task-2", assigneeHumanId: "human-3", dueDate: undefined },
      // No mention: the stored owner and due date are kept.
      { taskId: "task-3", assigneeHumanId: "human-9", dueDate: "2026-05-01" },
    ]);
  });
});

describe("task owners from unchanged mentions", () => {
  it("keeps the stored owner while the mention is the same as before", () => {
    const body = (humanId: string) => [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "Ship " },
          { type: "mention-@", attrs: { id: humanId, type: "human" } },
        ],
      },
    ];
    const content = (humanId: string) => ({
      type: "doc",
      content: [
        {
          type: "taskList",
          content: [
            {
              type: "taskItem",
              attrs: { status: "todo", taskId: "task-1", taskItemId: "i1" },
              content: body(humanId),
            },
          ],
        },
      ],
    });
    const previous = new Map([
      [
        "task-1",
        {
          taskId: "task-1",
          sourceId: "s",
          sourceType: "session_raw_note",
          sourceOrder: 0,
          status: "todo" as const,
          textPreview: "Ship",
          body: body("alice"),
          assigneeHumanId: "bob",
        },
      ],
    ]);
    const source = { type: "session_raw_note", id: "s" };

    expect(
      extractTasksFromContent(content("alice") as never, source, previous)[0]
        ?.assigneeHumanId,
    ).toBe("bob");
    expect(
      extractTasksFromContent(content("cara") as never, source, previous)[0]
        ?.assigneeHumanId,
    ).toBe("cara");
  });
});

describe("in-memory task storage", () => {
  it("reconciles tasks per source without duplicating other sources", () => {
    const storage = createInMemoryTaskStorage();

    const dailySource = { type: "daily_note", id: "2026-04-06" };
    const enhancedSource = { type: "enhanced_note", id: "enhanced-1" };

    storage.upsertTasksForSource(dailySource, [
      {
        taskId: "task-1",
        sourceId: dailySource.id,
        sourceType: dailySource.type,
        sourceOrder: 0,
        status: "todo",
        textPreview: "Daily task",
        body: [{ type: "paragraph" }],
      },
    ]);
    storage.upsertTasksForSource(enhancedSource, [
      {
        taskId: "task-2",
        sourceId: enhancedSource.id,
        sourceType: enhancedSource.type,
        sourceOrder: 0,
        status: "todo",
        textPreview: "Enhanced task",
        body: [{ type: "paragraph" }],
      },
    ]);
    storage.upsertTasksForSource(dailySource, [
      {
        taskId: "task-1",
        sourceId: dailySource.id,
        sourceType: dailySource.type,
        sourceOrder: 0,
        status: "done",
        textPreview: "Daily task updated",
        body: [{ type: "paragraph" }],
        dueDate: "2026-04-12",
      },
    ]);

    expect(storage.getTasksForSource(dailySource)).toMatchObject([
      {
        taskId: "task-1",
        status: "done",
        textPreview: "Daily task updated",
        dueDate: "2026-04-12",
      },
    ]);
    expect(storage.getTasksForSource(enhancedSource)).toMatchObject([
      {
        taskId: "task-2",
        status: "todo",
      },
    ]);
  });

  it("moves tasks to a new source with updated source order", () => {
    const storage = createInMemoryTaskStorage();
    const previousSource = { type: "daily_note", id: "2026-04-05" };
    const currentSource = { type: "daily_note", id: "2026-04-06" };

    storage.upsertTasksForSource(previousSource, [
      {
        taskId: "task-1",
        sourceId: previousSource.id,
        sourceType: previousSource.type,
        sourceOrder: 0,
        status: "todo",
        textPreview: "Carry me",
        body: [{ type: "paragraph" }],
      },
    ]);

    storage.moveTasksToSource(["task-1"], currentSource, 3);

    expect(storage.getTask("task-1")).toMatchObject({
      sourceId: currentSource.id,
      sourceType: currentSource.type,
      sourceOrder: 3,
    });
    expect(storage.getTasksForSource(previousSource)).toHaveLength(0);
    expect(storage.getTasksForSource(currentSource)).toMatchObject([
      {
        taskId: "task-1",
        sourceOrder: 3,
      },
    ]);
  });
});
