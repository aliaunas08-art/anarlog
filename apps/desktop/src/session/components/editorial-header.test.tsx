import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Tab } from "~/store/zustand/tabs";

const mocks = vi.hoisted(() => ({
  session: { created_at: "2026-11-14T17:38:00.000Z" } as {
    created_at?: string;
  } | null,
  event: null as Record<string, string | undefined> | null,
  audioTotal: 0,
  inPerson: false,
  timezone: undefined as string | undefined,
  participants: [] as Array<{
    id: string;
    humanId: string;
    name: string;
    email: string;
  }>,
}));

vi.mock("~/shared/config", () => ({
  useConfigValue: () => mocks.timezone,
}));

vi.mock("./title-input", () => ({
  TitleInput: ({ variant }: { variant?: string }) => (
    <input aria-label="Session title" data-variant={variant} />
  ),
}));

vi.mock("~/session/queries", () => ({
  useSession: () => mocks.session,
  useSessionParticipants: () => mocks.participants,
}));

vi.mock("~/session/hooks/useSessionEvent", () => ({
  useSessionEvent: () => mocks.event,
}));

vi.mock("~/audio-player", () => ({
  useAudioPlayer: () => ({
    timeStore: {
      subscribe: () => () => {},
      getSnapshot: () => ({ current: 0, total: mocks.audioTotal }),
    },
  }),
}));

vi.mock("~/stt/useStartListening", () => ({
  isSessionMarkedInPerson: () => mocks.inPerson,
}));

import {
  EditorialHeader,
  formatEyebrowDate,
  getDurationMinutes,
} from "./editorial-header";

const tab = {
  type: "sessions",
  id: "session-1",
  active: true,
  pinned: false,
  slotId: "slot-1",
  state: { view: null, autoStart: null },
} as Extract<Tab, { type: "sessions" }>;

function eyebrow() {
  return document.querySelector("[data-session-editorial-header] p")
    ?.textContent;
}

describe("EditorialHeader", () => {
  beforeEach(() => {
    mocks.session = { created_at: "2026-11-14T17:38:00.000Z" };
    mocks.event = null;
    mocks.audioTotal = 0;
    mocks.inPerson = false;
    mocks.timezone = undefined;
    mocks.participants = [];
  });

  afterEach(() => {
    cleanup();
  });

  it("renders the editable title in the editorial variant", () => {
    render(<EditorialHeader tab={tab} />);

    const title = screen.getByRole("textbox", { name: "Session title" });
    expect(title.getAttribute("data-variant")).toBe("editorial");
  });

  it("shows participants under the title and hides the row without any", () => {
    const { container, rerender } = render(<EditorialHeader tab={tab} />);
    expect(container.querySelector("[data-session-meta-row]")).toBeNull();

    mocks.participants = [
      { id: "p1", humanId: "h1", name: "Sam Lee", email: "" },
      { id: "p2", humanId: "h2", name: "Priya Shah", email: "" },
      { id: "p3", humanId: "", name: "", email: "jordan@example.com" },
    ];
    rerender(<EditorialHeader tab={tab} />);

    const row = container.querySelector("[data-session-meta-row]");
    expect(row?.textContent).toContain("Sam, Priya, jordan");
    expect(
      container.querySelectorAll("[data-session-meta-avatar]"),
    ).toHaveLength(3);
  });

  it("shows date, in person and recorded duration in the eyebrow", () => {
    mocks.inPerson = true;
    mocks.audioTotal = 32 * 60 + 10;
    render(<EditorialHeader tab={tab} />);

    const text = eyebrow() ?? "";
    expect(text).toMatch(/Nov 14/);
    expect(text).toContain(" · In person · 32 min");
  });

  it("names the meeting app and uses the event length when there is no audio", () => {
    mocks.event = {
      started_at: "2026-11-14T14:00:00.000Z",
      ended_at: "2026-11-14T14:45:00.000Z",
      meeting_link: "https://meet.google.com/abc-defg-hij",
    };
    render(<EditorialHeader tab={tab} />);

    expect(eyebrow()).toContain(" · Google Meet · 45 min");
  });

  it("dates the eyebrow in the app's configured timezone", () => {
    // 01:30 UTC on Nov 15 is still Nov 14 in Los Angeles.
    mocks.session = { created_at: "2026-11-15T01:30:00.000Z" };
    mocks.timezone = "America/Los_Angeles";
    render(<EditorialHeader tab={tab} />);

    expect(eyebrow()).toMatch(/Nov 14/);
  });

  it("omits the eyebrow parts it does not know", () => {
    mocks.session = null;
    render(<EditorialHeader tab={tab} />);

    expect(eyebrow()).toBeUndefined();
  });
});

describe("getDurationMinutes", () => {
  it("prefers the recorded audio length", () => {
    expect(
      getDurationMinutes({
        audioSeconds: 20,
        startedAt: "2026-11-14T14:00:00.000Z",
        endedAt: "2026-11-14T15:00:00.000Z",
      }),
    ).toBe(1);
  });

  it("returns null without audio or a complete event range", () => {
    expect(getDurationMinutes({ audioSeconds: 0 })).toBeNull();
    expect(
      getDurationMinutes({
        audioSeconds: 0,
        startedAt: "2026-11-14T14:00:00.000Z",
      }),
    ).toBeNull();
  });
});

describe("formatEyebrowDate", () => {
  it("falls back to the system timezone for an unknown one", () => {
    const date = new Date("2026-11-14T12:00:00.000Z");

    expect(formatEyebrowDate(date, "en-US", "Not/AZone")).toBe(
      formatEyebrowDate(date, "en-US"),
    );
  });
});
