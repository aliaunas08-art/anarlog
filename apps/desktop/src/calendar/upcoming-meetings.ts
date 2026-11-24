import { useEffect, useMemo, useRef, useState } from "react";

import { parseEventInstant } from "@anlg/utils";

import { useIgnoredEvents } from "~/calendar/ignored-events";
import { useLiveQuery } from "~/db";

export type UpcomingMeetingRow = {
  id: string;
  tracking_id_event: string;
  recurrence_series_id: string;
  title: string;
  started_at: string;
  meeting_link: string;
};

const UPCOMING_SQL = `
  SELECT e.id, e.tracking_id_event, e.recurrence_series_id, e.title,
    e.started_at, e.meeting_link
  FROM events e
  JOIN calendars c ON c.id = e.calendar_id
  WHERE c.enabled = 1
    AND c.deleted_at IS NULL
    AND e.deleted_at IS NULL
    AND e.is_all_day = 0
    AND julianday(e.started_at) > julianday(?)
    AND julianday(e.started_at) < julianday(?)
  ORDER BY julianday(e.started_at)
  LIMIT 10
`;

const HOUR_MS = 60 * 60 * 1000;
const UPCOMING_WINDOW_MS = 12 * HOUR_MS;
// A meeting that started a few minutes ago can still be joined and recorded.
const STARTED_GRACE_MS = 10 * 60 * 1000;

/** Wall-clock time, floored to the minute and refreshed every 30s. */
export function useMinuteNow() {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);
  return Math.floor(now / 60_000) * 60_000;
}

/** Keeps the last loaded value so re-keyed live queries don't flash empty. */
function useStable<T>(value: T | undefined): T | undefined {
  const last = useRef(value);
  if (value !== undefined) last.current = value;
  return last.current;
}

/**
 * Calendar meetings starting in the next 12 hours (or a few minutes ago), in
 * start order, without ignored events.
 */
export function useUpcomingMeetings(now: number): UpcomingMeetingRow[] {
  const { isIgnored } = useIgnoredEvents();
  // Query an hour-aligned window so params (and the live query) change hourly,
  // then filter on the client every minute.
  const hour = Math.floor(now / HOUR_MS) * HOUR_MS;
  const params = useMemo(
    () => [
      new Date(hour - STARTED_GRACE_MS).toISOString(),
      new Date(hour + HOUR_MS + UPCOMING_WINDOW_MS).toISOString(),
    ],
    [hour],
  );
  const { data } = useLiveQuery<UpcomingMeetingRow, UpcomingMeetingRow[]>({
    sql: UPCOMING_SQL,
    params,
    mapRows: (rows) => rows,
  });
  const rows = useStable(data) ?? [];
  return rows.filter((row) => {
    const start = parseEventInstant(row.started_at)?.getTime();
    return (
      start !== undefined &&
      start > now - STARTED_GRACE_MS &&
      start < now + UPCOMING_WINDOW_MS &&
      !isIgnored(row.tracking_id_event, row.recurrence_series_id)
    );
  });
}

/** "Google Meet" / "Zoom" / "Teams" from a meeting link, else null. */
export function meetingProviderLabel(link: string): string | null {
  if (!link) return null;
  let host = "";
  try {
    host = new URL(link).hostname;
  } catch {
    return null;
  }
  if (host.endsWith("meet.google.com")) return "Google Meet";
  if (host.endsWith("zoom.us")) return "Zoom";
  if (host.endsWith("teams.microsoft.com") || host.endsWith("teams.live.com"))
    return "Teams";
  if (host.endsWith("webex.com")) return "Webex";
  return null;
}
