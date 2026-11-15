import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  today: "2026-11-15",
  timezone: "UTC" as string | undefined,
  weekStartsOn: 0 as 0 | 1,
  timelineSessionsTable: {} as Record<string, Record<string, unknown>>,
}));

vi.mock("@lingui/react/macro", () => ({
  Trans: ({ children }: { children?: ReactNode }) => <>{children}</>,
  useLingui: () => ({
    i18n: { locale: "en-US" },
    t: (strings: TemplateStringsArray, ...values: unknown[]) =>
      strings.reduce(
        (message, part, index) =>
          `${message}${part}${index < values.length ? String(values[index]) : ""}`,
        "",
      ),
  }),
}));

vi.mock("~/shared/config", () => ({
  useConfigValue: () => mocks.timezone,
}));

vi.mock("~/shared/hooks/useCurrentDay", () => ({
  useCurrentDay: () => mocks.today,
}));

vi.mock("~/calendar/hooks", () => ({
  useWeekStartsOn: () => mocks.weekStartsOn,
}));

vi.mock("~/calendar/queries", () => ({
  useTimelineSessionsTable: () => mocks.timelineSessionsTable,
}));

import { SidebarDateFilter } from "./date-filter";
import { resetSidebarNotes, useSidebarNotes } from "./note-filter";

function day(dayKey: string) {
  const cell = document.querySelector<HTMLElement>(`[data-day="${dayKey}"]`);
  if (!cell) throw new Error(`No cell for ${dayKey}`);
  return cell;
}

function hasDot(dayKey: string) {
  return within(day(dayKey)).queryByTestId("date-filter-dot") !== null;
}

describe("SidebarDateFilter", () => {
  beforeEach(() => {
    resetSidebarNotes();
    mocks.today = "2026-11-15";
    mocks.timezone = "UTC";
    mocks.weekStartsOn = 0;
    mocks.timelineSessionsTable = {
      a: { title: "Launch", created_at: "2026-11-14T17:38:00Z" },
      b: { title: "Weather", created_at: "2026-11-14T18:09:00Z" },
      c: { title: "Early", created_at: "2026-11-03T10:00:00Z" },
      // The linked event's start wins over created_at, as in the timeline.
      d: {
        title: "Event note",
        created_at: "2026-11-09T10:00:00Z",
        event_json: JSON.stringify({ started_at: "2026-11-10T09:00:00Z" }),
      },
      e: { title: "October", created_at: "2026-10-20T10:00:00Z" },
      f: {
        title: "Work",
        created_at: "2026-11-05T10:00:00Z",
        folder_id: "work",
      },
    };
  });

  afterEach(() => {
    cleanup();
    resetSidebarNotes();
  });

  it("renders the current month as a grid", () => {
    render(<SidebarDateFilter />);

    expect(screen.getByTestId("date-filter-month").textContent).toBe(
      "November 2026",
    );
    const grid = screen.getByRole("grid", { name: "November 2026" });
    expect(within(grid).getAllByRole("columnheader")).toHaveLength(7);
    // Six full weeks, padded with October and December days.
    expect(grid.querySelectorAll("[data-day]")).toHaveLength(42);
    expect(
      grid.querySelectorAll("[data-day]:not([data-outside])"),
    ).toHaveLength(30);
    expect(day("2026-12-12").hasAttribute("data-outside")).toBe(true);
    // November 1, 2026 is a Sunday, so it opens the first row.
    const firstRow = within(grid).getAllByRole("row")[1];
    expect(firstRow.querySelector("[data-day]")?.getAttribute("data-day")).toBe(
      "2026-11-01",
    );
    expect(day("2026-11-15").getAttribute("aria-current")).toBe("date");
  });

  it("marks days that have notes", () => {
    render(<SidebarDateFilter />);

    expect(hasDot("2026-11-14")).toBe(true);
    expect(hasDot("2026-11-03")).toBe(true);
    expect(hasDot("2026-11-10")).toBe(true);
    expect(hasDot("2026-11-09")).toBe(false);
    expect(hasDot("2026-11-15")).toBe(false);
    expect(day("2026-11-14").getAttribute("aria-label")).toContain("2 notes");
  });

  it("scopes dots to the folder filter", () => {
    render(<SidebarDateFilter folderFilter="work" />);

    expect(hasDot("2026-11-05")).toBe(true);
    expect(hasDot("2026-11-14")).toBe(false);
  });

  it("selects a day, summarises it, and clears with Any date", () => {
    render(<SidebarDateFilter />);

    fireEvent.click(day("2026-11-14"));

    expect(useSidebarNotes.getState().dateFilter).toBe("2026-11-14");
    expect(day("2026-11-14").getAttribute("aria-selected")).toBe("true");
    expect(screen.getByText(/Sat, Nov 14 · 2 notes/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Any date" }));

    expect(useSidebarNotes.getState().dateFilter).toBeNull();
    expect(day("2026-11-14").getAttribute("aria-selected")).toBe("false");
    expect(screen.queryByRole("button", { name: "Any date" })).toBeNull();
  });

  it("clears when the selected day is clicked again", () => {
    render(<SidebarDateFilter />);

    fireEvent.click(day("2026-11-03"));
    expect(screen.getByText(/1 note$/)).toBeTruthy();
    fireEvent.click(day("2026-11-03"));

    expect(useSidebarNotes.getState().dateFilter).toBeNull();
  });

  it("selecting an adjacent-month day pages to that month", () => {
    render(<SidebarDateFilter />);

    fireEvent.click(day("2026-12-03"));

    expect(useSidebarNotes.getState().dateFilter).toBe("2026-12-03");
    expect(screen.getByTestId("date-filter-month").textContent).toBe(
      "December 2026",
    );
    expect(day("2026-12-03").hasAttribute("data-outside")).toBe(false);
  });

  it("navigates between months", () => {
    render(<SidebarDateFilter />);

    fireEvent.click(screen.getByRole("button", { name: "Previous month" }));
    expect(screen.getByTestId("date-filter-month").textContent).toBe(
      "October 2026",
    );
    expect(
      document.querySelectorAll("[data-day]:not([data-outside])"),
    ).toHaveLength(31);
    expect(hasDot("2026-10-20")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    fireEvent.click(screen.getByRole("button", { name: "Next month" }));
    expect(screen.getByTestId("date-filter-month").textContent).toBe(
      "December 2026",
    );
  });

  it("supports arrow keys, Enter and Escape", () => {
    render(<SidebarDateFilter />);
    const grid = screen.getByRole("grid");

    expect(day("2026-11-15").tabIndex).toBe(0);
    act(() => day("2026-11-15").focus());
    fireEvent.keyDown(grid, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(day("2026-11-14"));
    expect(day("2026-11-14").tabIndex).toBe(0);

    fireEvent.keyDown(grid, { key: "Enter" });
    expect(useSidebarNotes.getState().dateFilter).toBe("2026-11-14");

    // Crossing the month boundary pages the calendar.
    fireEvent.keyDown(grid, { key: "ArrowUp" });
    fireEvent.keyDown(grid, { key: "ArrowUp" });
    expect(document.activeElement).toBe(day("2026-10-31"));
    expect(screen.getByTestId("date-filter-month").textContent).toBe(
      "October 2026",
    );

    fireEvent.keyDown(grid, { key: "Escape" });
    expect(useSidebarNotes.getState().dateFilter).toBeNull();
  });

  it("starts the week on Monday when configured", () => {
    mocks.weekStartsOn = 1;
    render(<SidebarDateFilter />);

    const headers = screen.getAllByRole("columnheader");
    expect(headers[0].getAttribute("aria-label")).toBe("Monday");
    // Nov 1 is a Sunday: six October days precede it.
    const firstRow = screen.getAllByRole("row")[1];
    expect(
      firstRow.querySelectorAll("[data-day]:not([data-outside])"),
    ).toHaveLength(1);
    expect(firstRow.querySelector("[data-day]")?.getAttribute("data-day")).toBe(
      "2026-10-26",
    );
  });
});

describe("date filter store", () => {
  afterEach(() => resetSidebarNotes());

  it("hiding the calendar clears the picked day", () => {
    useSidebarNotes.getState().setDateFilter("2026-11-14");
    expect(useSidebarNotes.getState().showDateFilter).toBe(true);

    useSidebarNotes.getState().setShowDateFilter(false);
    expect(useSidebarNotes.getState().dateFilter).toBeNull();
  });
});
