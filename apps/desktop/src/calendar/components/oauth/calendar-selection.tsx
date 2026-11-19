import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";

import { useSync } from "../context";

import {
  type CalendarGroup,
  type CalendarItem,
  CalendarSelection,
} from "~/calendar/components/calendar-selection";
import type { CalendarProvider } from "~/calendar/components/shared";
import { setCalendarEnabled, useCalendarRows } from "~/calendar/queries";

export function OAuthCalendarSelection({
  groups,
  onToggle,
  onRefresh,
  isLoading,
}: {
  groups: CalendarGroup[];
  onToggle: (
    calendar: CalendarItem,
    enabled: boolean,
  ) => void | Promise<unknown>;
  onRefresh?: () => void;
  isLoading: boolean;
}) {
  return (
    <CalendarSelection
      groups={groups}
      onToggle={onToggle}
      onRefresh={onRefresh}
      isLoading={isLoading}
    />
  );
}

export function useOAuthCalendarSelection(config: CalendarProvider) {
  const queryClient = useQueryClient();
  const calendars = useCalendarRows(config.id);
  const { cancelDebouncedSync, status, scheduleDebouncedSync, scheduleSync } =
    useSync();

  const { groups, connectionSourceMap } = useMemo(() => {
    const sourceMap = new Map<string, string>();

    for (const cal of calendars) {
      if (cal.source && cal.connection_id) {
        sourceMap.set(cal.connection_id, cal.source);
      }
    }

    const nonNullSources = new Set(
      calendars
        .map((cal) => {
          if (cal.source) {
            return cal.source;
          }
          if (cal.connection_id) {
            return sourceMap.get(cal.connection_id);
          }
          return undefined;
        })
        .filter(Boolean),
    );
    const singleSource =
      nonNullSources.size === 1 ? ([...nonNullSources][0] as string) : null;

    const grouped = new Map<
      string,
      { connectionId?: string; sourceName: string; calendars: CalendarItem[] }
    >();

    for (const cal of calendars) {
      const connectionId = cal.connection_id || undefined;
      const source =
        cal.source ||
        (connectionId ? sourceMap.get(connectionId) : undefined) ||
        singleSource ||
        config.displayName;
      // One group per account and owner: two accounts can share an owner
      // name, and one account can hold several owners' calendars.
      const groupKey = `${connectionId ?? ""}\u0000${source}`;
      if (!grouped.has(groupKey)) {
        grouped.set(groupKey, {
          connectionId,
          sourceName: source,
          calendars: [],
        });
      }
      const group = grouped.get(groupKey)!;
      if (!group.connectionId && connectionId) {
        group.connectionId = connectionId;
      }
      group.calendars.push({
        id: cal.id,
        title: cal.name ?? "Untitled",
        color: cal.color || "#4285f4",
        enabled: cal.enabled ?? false,
      });
    }

    return {
      groups: Array.from(grouped.values()).map((group) => ({
        id: group.connectionId,
        sourceName: group.sourceName,
        calendars: group.calendars,
      })),
      connectionSourceMap: sourceMap,
    };
  }, [calendars, config.displayName]);

  const handleToggle = useCallback(
    (calendar: CalendarItem, enabled: boolean) =>
      setCalendarEnabled(calendar.id, enabled)
        .then(scheduleDebouncedSync)
        .catch((error: unknown) => {
          console.error("[calendar] failed to update calendar", error);
          throw error;
        }),
    [scheduleDebouncedSync],
  );

  const handleRefresh = useCallback(() => {
    cancelDebouncedSync();
    void queryClient.invalidateQueries({
      queryKey: ["integration-status"],
    });
    scheduleSync();
  }, [cancelDebouncedSync, queryClient, scheduleSync]);

  return {
    groups,
    connectionSourceMap,
    handleRefresh,
    handleToggle,
    isLoading: status === "syncing",
  };
}
