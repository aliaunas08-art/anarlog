import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TaskRow } from "./tasks-data";

const mocks = vi.hoisted(() => ({
  rows: [] as TaskRow[],
  setActionItemStatus: vi.fn(),
  openCurrent: vi.fn(),
}));

vi.mock("~/db", () => ({
  useLiveQuery: ({ mapRows }: { mapRows: (rows: unknown[]) => unknown }) => ({
    data: mapRows(mocks.rows),
    isLoading: false,
  }),
}));

vi.mock("~/editor-bridge/task-storage", () => ({
  setActionItemStatus: mocks.setActionItemStatus,
}));

vi.mock("~/shared/owner-user", () => ({
  useOwnerUserId: () => "user-1",
}));

vi.mock("~/shared/main", () => ({
  StandardContentWrapper: ({
    children,
    floatingButton,
  }: {
    children: ReactNode;
    floatingButton?: ReactNode;
  }) => (
    <div>
      {children}
      {floatingButton}
    </div>
  ),
}));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (selector: (state: { openCurrent: unknown }) => unknown) =>
    selector({ openCurrent: mocks.openCurrent }),
}));

import { TabContentTasks, TasksView } from "./tasks-view";

const NOW = new Date(2026, 10, 15, 10, 0);

function row(overrides: Partial<TaskRow>): TaskRow {
  return {
    id: "t",
    status: "todo",
    text: "Task",
    due_at: "",
    assignee_human_id: "",
    owner_name: null,
    owner_email: null,
    session_id: "s1",
    session_title: "Product sync",
    session_date: "2026-11-12T10:00:00.000Z",
    source_order: 0,
    updated_at: "2026-11-12T10:00:00.000Z",
    ...overrides,
  };
}

describe("TasksView", () => {
  beforeEach(() => {
    mocks.rows = [
      row({
        id: "a",
        text: "Fix step 3 copy",
        due_at: "2026-11-15",
        assignee_human_id: "h1",
        owner_name: "Jordan",
      }),
      row({
        id: "b",
        text: "Send the deck",
        due_at: "2026-11-20",
        session_id: "s2",
        session_title: "March Launch Planning",
      }),
      row({ id: "c", text: "Review pricing" }),
      row({ id: "d", text: "Book venue", status: "done" }),
    ];
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("renders open rows grouped by due date", () => {
    render(<TasksView view="all" now={NOW} />);

    expect(screen.getAllByTestId("task-row")).toHaveLength(3);
    const week = screen.getByTestId("task-group-due:this_week");
    expect(within(week).getByText("This week")).toBeTruthy();
    expect(within(week).getByText("Fix step 3 copy")).toBeTruthy();
    expect(within(week).getByText("Jordan")).toBeTruthy();
    expect(within(week).getByText("Today")).toBeTruthy();
    expect(
      within(screen.getByTestId("task-group-due:later")).getByText(
        "Send the deck",
      ),
    ).toBeTruthy();
    const noDate = screen.getByTestId("task-group-due:no_date");
    expect(within(noDate).getByText("Review pricing")).toBeTruthy();
    expect(within(noDate).getByText("No owner")).toBeTruthy();
    expect(screen.queryByText("Book venue")).toBeNull();
  });

  it("switches views with the segmented control", () => {
    render(<TabContentTasks />);

    fireEvent.click(screen.getByRole("tab", { name: "By person" }));
    expect(screen.getByTestId("task-group-person:h1")).toBeTruthy();
    expect(screen.getByTestId("task-group-person:")).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "By meeting" }));
    expect(screen.getByTestId("task-group-meeting:s1")).toBeTruthy();
    expect(screen.getByTestId("task-group-meeting:s2")).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "Done" }));
    expect(screen.getByRole("tab", { name: "Done" }).ariaSelected).toBe("true");
    expect(screen.getAllByTestId("task-row")).toHaveLength(1);
    expect(screen.getByText("Book venue")).toBeTruthy();
  });

  it("toggles done through the action item mutation", () => {
    render(<TasksView view="all" now={NOW} />);

    const checkbox = screen.getByRole("checkbox", { name: "Review pricing" });
    fireEvent.click(checkbox);
    expect(mocks.setActionItemStatus).toHaveBeenCalledWith(
      "c",
      "done",
      "user-1",
    );
    // Stays in place, checked, until the view changes.
    expect(
      (
        screen.getByRole("checkbox", {
          name: "Review pricing",
        }) as HTMLInputElement
      ).checked,
    ).toBe(true);
  });

  it("opens the meeting from the row", () => {
    render(<TasksView view="all" now={NOW} />);

    fireEvent.click(
      screen.getByRole("button", { name: "March Launch Planning" }),
    );
    expect(mocks.openCurrent).toHaveBeenCalledWith({
      type: "sessions",
      id: "s2",
    });
  });

  it("shows a friendly empty state", () => {
    mocks.rows = [];
    render(<TasksView view="all" now={NOW} />);

    expect(screen.getByTestId("tasks-empty")).toBeTruthy();
    expect(screen.getByText("No tasks yet")).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
  });
});
