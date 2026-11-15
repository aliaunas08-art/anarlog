import { describe, expect, it } from "vitest";

import {
  getDueBucket,
  getWeekRange,
  groupTasks,
  mapTaskRows,
  parseDueDate,
  type TaskItem,
} from "./tasks-data";

// Sunday, Nov 15 2026 (week runs Mon Nov 9 – Sun Nov 15).
const NOW = new Date(2026, 10, 15, 10, 0);

function task(overrides: Partial<TaskItem>): TaskItem {
  return {
    id: "t",
    text: "Task",
    done: false,
    dueAt: "",
    ownerId: "",
    ownerName: "",
    sessionId: "s1",
    sessionTitle: "Product sync",
    sessionDate: "2026-11-12T10:00:00.000Z",
    sourceOrder: 0,
    updatedAt: "",
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
        updated_at: "",
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
        updated_at: null,
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
        updated_at: "",
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

  it("parses only real dates", () => {
    expect(parseDueDate("")).toBeNull();
    expect(parseDueDate("End of week")).toBeNull();
    expect(parseDueDate("2026-11-20")?.getDate()).toBe(20);
    expect(parseDueDate("2026-11-20T09:00:00Z")).not.toBeNull();
  });

  it("buckets due dates by Monday-based week", () => {
    const { start, end } = getWeekRange(NOW);
    expect(start.getDate()).toBe(9);
    expect(end.getDate()).toBe(16);
    expect(getDueBucket("2026-11-15", NOW)).toBe("this_week");
    expect(getDueBucket("2026-11-01", NOW)).toBe("this_week");
    expect(getDueBucket("2026-11-16", NOW)).toBe("later");
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
      task({ id: "4", done: true, updatedAt: "2026-11-10" }),
      task({ id: "5", done: true, updatedAt: "2026-11-11" }),
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
