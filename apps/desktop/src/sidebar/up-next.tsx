import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";

import { cn, parseEventInstant } from "@anlg/utils";

import {
  meetingProviderLabel,
  useMinuteNow,
  useUpcomingMeetings,
} from "~/calendar/upcoming-meetings";
import { getOrCreateSessionForEventId } from "~/session/queries";
import { useTabs } from "~/store/zustand/tabs";

/** Board "Up next": the next two calendar meetings; hidden when there are none. */
export function SidebarUpNext() {
  const { t, i18n } = useLingui();
  const now = useMinuteNow();
  const meetings = useUpcomingMeetings(now).slice(0, 2);
  const openCurrent = useTabs((state) => state.openCurrent);
  const [openingId, setOpeningId] = useState<string | null>(null);

  if (meetings.length === 0) return null;

  const open = (id: string, title: string) => {
    if (openingId) return;
    setOpeningId(id);
    void getOrCreateSessionForEventId(id, title)
      .then((sessionId) => openCurrent({ id: sessionId, type: "sessions" }))
      .catch((error) =>
        console.error("[sidebar] failed to open meeting", error),
      )
      .finally(() => setOpeningId(null));
  };

  return (
    <nav
      aria-label={t`Up next`}
      data-sidebar-up-next
      className="flex shrink-0 flex-col gap-0.5 px-2 pb-2"
    >
      <span className="px-2 pt-1 pb-1 text-[11px] font-semibold whitespace-nowrap text-[var(--cf-secondary)]">
        <Trans>Up next</Trans>
      </span>
      {meetings.map((meeting, index) => {
        const title = meeting.title || t`Untitled meeting`;
        const start = parseEventInstant(meeting.started_at);
        const time = start
          ? start.toLocaleTimeString(i18n.locale, {
              hour: "numeric",
              minute: "2-digit",
            })
          : "";
        const provider = meetingProviderLabel(meeting.meeting_link);
        return (
          <button
            key={meeting.id}
            type="button"
            onClick={() => open(meeting.id, title)}
            className="cf-motion flex min-w-0 items-stretch gap-2 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-[var(--cf-fill)]"
          >
            <span
              aria-hidden="true"
              className={cn([
                "w-[3px] shrink-0 rounded-[2px]",
                index === 0
                  ? "bg-[var(--cf-accent)]"
                  : "bg-[var(--cf-separator-strong)]",
              ])}
            />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-[13px] leading-[18px] font-medium text-[var(--cf-label)]">
                {title}
              </span>
              <span className="truncate text-[11px] leading-4 text-[var(--cf-secondary)]">
                {provider ? `${time} · ${provider}` : time}
              </span>
            </span>
          </button>
        );
      })}
    </nav>
  );
}
