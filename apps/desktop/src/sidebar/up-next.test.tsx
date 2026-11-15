import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  rows: [] as Array<Record<string, string>>,
  ignored: new Set<string>(),
  openCurrent: vi.fn(),
  getOrCreateSessionForEventId: vi.fn(async () => "event-session"),
}));

vi.mock("~/db", () => ({
  useLiveQuery: ({ mapRows }: { mapRows: (rows: unknown[]) => unknown }) => ({
    data: mapRows(mocks.rows),
  }),
}));
vi.mock("~/calendar/ignored-events", () => ({
  useIgnoredEvents: () => ({
    isIgnored: (trackingId: string) => mocks.ignored.has(trackingId),
  }),
}));
vi.mock("~/session/queries", () => ({
  getOrCreateSessionForEventId: mocks.getOrCreateSessionForEventId,
}));
vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (
    selector: (state: { openCurrent: typeof mocks.openCurrent }) => unknown,
  ) => selector({ openCurrent: mocks.openCurrent }),
}));

import { SidebarUpNext } from "./up-next";

const inMinutes = (minutes: number) =>
  new Date(Date.now() + minutes * 60_000).toISOString();

const event = (id: string, title: string, minutes: number, link = "") => ({
  id,
  tracking_id_event: `trk-${id}`,
  recurrence_series_id: "",
  title,
  started_at: inMinutes(minutes),
  meeting_link: link,
});

describe("SidebarUpNext", () => {
  afterEach(() => {
    cleanup();
    mocks.rows = [];
    mocks.ignored = new Set();
    vi.clearAllMocks();
  });

  it("renders nothing without upcoming meetings", () => {
    const { container } = render(<SidebarUpNext />);
    expect(container.innerHTML).toBe("");
  });

  it("lists the next two meetings and opens one", async () => {
    mocks.rows = [
      event("a", "Product sync", 20, "https://meet.google.com/abc"),
      event("b", "1:1 with Priya", 90),
      event("c", "Later", 200),
    ];
    render(<SidebarUpNext />);
    expect(screen.getByRole("navigation", { name: "Up next" })).toBeTruthy();
    expect(screen.getByText("Product sync")).toBeTruthy();
    expect(screen.getByText(/· Google Meet$/)).toBeTruthy();
    expect(screen.getByText("1:1 with Priya")).toBeTruthy();
    expect(screen.queryByText("Later")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Product sync/ }));
    expect(mocks.getOrCreateSessionForEventId).toHaveBeenCalledWith(
      "a",
      "Product sync",
    );
    await vi.waitFor(() =>
      expect(mocks.openCurrent).toHaveBeenCalledWith({
        id: "event-session",
        type: "sessions",
      }),
    );
  });

  it("skips ignored meetings", () => {
    mocks.rows = [event("a", "Ignored", 20)];
    mocks.ignored = new Set(["trk-a"]);
    const { container } = render(<SidebarUpNext />);
    expect(container.innerHTML).toBe("");
  });
});
