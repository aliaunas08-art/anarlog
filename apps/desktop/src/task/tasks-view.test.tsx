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
  setActionItemOwner: vi.fn(),
  setActionItemDue: vi.fn(),
  openCurrent: vi.fn(),
}));

vi.mock("~/db", () => ({
  useLiveQuery: ({ mapRows }: { mapRows: (rows: unknown[]) => unknown }) => ({
    data: mapRows(mocks.rows),
    isLoading: false,
  }),
}));

vi.mock("~/editor-bridge/task-storage", () => ({
  resolvedTaskSessionIdSql: () => "''",
  setActionItemStatus: mocks.setActionItemStatus,
  setActionItemOwner: mocks.setActionItemOwner,
  setActionItemDue: mocks.setActionItemDue,
}));

vi.mock("~/contacts/queries", () => ({
  useHumans: () => [
    { id: "h1", name: "Jordan", email: "jordan@example.com" },
    { id: "h2", name: "Ana Lee", email: "ana@example.com" },
    { id: "h3", name: "", email: "sam@example.com" },
  ],
}));

vi.mock("~/calendar/hooks", () => ({
  useTimezone: () => "UTC",
  useWeekStartsOn: () => 1,
}));

vi.mock("~/shared/hooks/useCurrentDay", () => ({
  useCurrentDay: () => "2026-11-15",
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
    done_at: "2026-11-12T10:00:00.000Z",
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
    render(<TasksView view="all" />);

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

    fireEvent.click(screen.getByRole("radio", { name: "By person" }));
    expect(screen.getByTestId("task-group-person:h1")).toBeTruthy();
    expect(screen.getByTestId("task-group-person:")).toBeTruthy();

    fireEvent.keyDown(screen.getByRole("radio", { name: "By person" }), {
      key: "ArrowRight",
    });
    expect(screen.getByTestId("task-group-meeting:s1")).toBeTruthy();
    expect(screen.getByTestId("task-group-meeting:s2")).toBeTruthy();
    expect(document.activeElement).toBe(
      screen.getByRole("radio", { name: "By meeting" }),
    );

    fireEvent.click(screen.getByRole("radio", { name: "Done" }));
    expect(
      (screen.getByRole("radio", { name: "Done" }) as HTMLElement).ariaChecked,
    ).toBe("true");
    expect(screen.getAllByTestId("task-row")).toHaveLength(1);
    expect(screen.getByText("Book venue")).toBeTruthy();
  });

  it("toggles done through the action item mutation", () => {
    render(<TasksView view="all" />);

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

  it("restores an in-progress task when it is unchecked again", () => {
    mocks.rows = [row({ id: "p", text: "Draft memo", status: "in_progress" })];
    render(<TasksView view="all" />);

    const checkbox = screen.getByRole("checkbox", { name: "Draft memo" });
    fireEvent.click(checkbox);
    fireEvent.click(checkbox);
    expect(mocks.setActionItemStatus).toHaveBeenLastCalledWith(
      "p",
      "in_progress",
      "user-1",
    );
  });

  it("opens the meeting from the row", () => {
    render(<TasksView view="all" />);

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
    render(<TasksView view="all" />);

    expect(screen.getByTestId("tasks-empty")).toBeTruthy();
    expect(screen.getByText("No tasks yet")).toBeTruthy();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("sets the owner from the Owner cell popover", async () => {
    render(<TasksView view="all" />);

    fireEvent.click(
      screen.getByRole("button", { name: "Owner of Review pricing: No owner" }),
    );
    const search = await screen.findByRole("combobox", {
      name: "Search people",
    });
    fireEvent.change(search, { target: { value: "sam@" } });
    expect(screen.queryByRole("option", { name: /Ana Lee/ })).toBeNull();
    fireEvent.keyDown(search, { key: "Enter" });
    expect(mocks.setActionItemOwner).toHaveBeenCalledWith("c", "h3", "user-1");

    fireEvent.click(
      screen.getByRole("button", { name: "Owner of Fix step 3 copy: Jordan" }),
    );
    fireEvent.click(await screen.findByRole("option", { name: "No owner" }));
    expect(mocks.setActionItemOwner).toHaveBeenLastCalledWith(
      "a",
      "",
      "user-1",
    );
  });

  it("sets and clears the due date from the Due cell popover", async () => {
    render(<TasksView view="all" />);

    fireEvent.click(
      screen.getByRole("button", { name: /^Due date of Review pricing: / }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /^Tomorrow/ }));
    expect(mocks.setActionItemDue).toHaveBeenCalledWith(
      "c",
      "2026-11-16",
      "user-1",
    );

    fireEvent.click(
      screen.getByRole("button", { name: /^Due date of Review pricing: / }),
    );
    fireEvent.click(await screen.findByRole("button", { name: /^Next week/ }));
    expect(mocks.setActionItemDue).toHaveBeenLastCalledWith(
      "c",
      "2026-11-16",
      "user-1",
    );

    fireEvent.click(
      screen.getByRole("button", { name: /^Due date of Send the deck: / }),
    );
    fireEvent.click(
      await screen.findByRole("gridcell", {
        name: "Wednesday, November 25, 2026",
      }),
    );
    expect(mocks.setActionItemDue).toHaveBeenLastCalledWith(
      "b",
      "2026-11-25",
      "user-1",
    );

    fireEvent.click(
      screen.getByRole("button", { name: /^Due date of Send the deck: / }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Clear" }));
    expect(mocks.setActionItemDue).toHaveBeenLastCalledWith("b", "", "user-1");
  });

  it("labels the cells with their current value", () => {
    render(<TasksView view="all" />);

    expect(
      screen.getByRole("button", {
        name: "Due date of Fix step 3 copy: Today",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", {
        name: "Due date of Review pricing: No date",
      }),
    ).toBeTruthy();
  });

  it("clears an owner whose contact no longer exists", async () => {
    mocks.rows = [
      row({ id: "e", text: "Orphaned", assignee_human_id: "gone" }),
    ];
    render(<TasksView view="all" />);

    fireEvent.click(
      screen.getByRole("button", { name: "Owner of Orphaned: No owner" }),
    );
    fireEvent.click(await screen.findByRole("option", { name: "No owner" }));
    expect(mocks.setActionItemOwner).toHaveBeenCalledWith("e", "", "user-1");
  });

  it("moves through the calendar with one tab stop and arrow keys", async () => {
    render(<TasksView view="all" />);

    fireEvent.click(
      screen.getByRole("button", { name: /^Due date of Fix step 3 copy: / }),
    );
    const grid = await screen.findByRole("grid");
    const focusable = within(grid)
      .getAllByRole("gridcell")
      .filter((cell) => cell.tabIndex === 0);
    expect(focusable.map((cell) => cell.dataset.day)).toEqual(["2026-11-15"]);

    focusable[0]!.focus();
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    fireEvent.keyDown(grid, { key: "ArrowRight" });
    expect(document.activeElement?.getAttribute("data-day")).toBe("2026-11-23");
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(mocks.setActionItemDue).toHaveBeenCalledWith(
      "a",
      "2026-11-23",
      "user-1",
    );
  });
});
