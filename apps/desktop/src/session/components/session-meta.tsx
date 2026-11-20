import { useLingui } from "@lingui/react/macro";
import { useLayoutEffect, useMemo, useRef, useState } from "react";

import { avatarInitials } from "@anlg/ui/lib/avatar";
import { cn } from "@anlg/utils";

import { useSessionParticipants } from "~/session/queries";

// Apple system colours (board avatars: indigo, orange, teal…).
const AVATAR_COLORS = [
  "#5E5CE6",
  "#FF9F0A",
  "#30B0C7",
  "#FF375F",
  "#32D74B",
  "#BF5AF2",
  "#0A84FF",
  "#AC8E68",
];
const MAX_AVATARS = 3;
const MAX_NAMES = 3;

export type MetaParticipant = { id: string; name: string };

export function useMetaParticipants(sessionId: string): MetaParticipant[] {
  const participants = useSessionParticipants(sessionId);
  return useMemo(() => {
    const seen = new Set<string>();
    const result: MetaParticipant[] = [];
    for (const participant of participants) {
      const name =
        participant.name.trim() || participant.email.split("@")[0]?.trim();
      if (!name) continue;
      const key = participant.humanId || name.toLocaleLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      result.push({ id: participant.humanId || participant.id, name });
    }
    return result;
  }, [participants]);
}

export function colorForParticipant(id: string) {
  let hash = 0;
  for (let index = 0; index < id.length; index += 1) {
    hash = (hash * 31 + id.charCodeAt(index)) | 0;
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

export function formatParticipantNames(
  participants: MetaParticipant[],
  more: (count: number) => string,
) {
  const names = participants
    .slice(0, MAX_NAMES)
    .map((participant) => participant.name.split(/\s+/)[0] ?? participant.name);
  const rest = participants.length - names.length;
  return rest > 0 ? [...names, more(rest)].join(", ") : names.join(", ");
}

/**
 * Board meta row under the title: overlapping initials avatars and first
 * names. Renders nothing without participants.
 */
export function SessionMetaRow({
  participants,
  className,
  style,
}: {
  participants: MetaParticipant[];
  className?: string;
  style?: React.CSSProperties;
}) {
  const { t } = useLingui();
  if (participants.length === 0) return null;

  return (
    <div
      data-session-meta-row
      style={style}
      className={cn([
        "flex h-[18px] min-w-0 items-center gap-1.5 text-xs whitespace-nowrap text-[var(--cf-secondary)] select-none",
        className,
      ])}
    >
      <span aria-hidden="true" className="flex shrink-0">
        {participants.slice(0, MAX_AVATARS).map((participant, index) => (
          <span
            key={participant.id}
            data-session-meta-avatar
            className={cn([
              "flex size-[18px] items-center justify-center rounded-[50%] text-[9px] leading-none font-bold text-white",
              "shadow-[0_0_0_2px_var(--cf-window)]",
              index > 0 && "-ml-1",
            ])}
            style={{ background: colorForParticipant(participant.id) }}
          >
            {avatarInitials(participant.name).slice(0, 1)}
          </span>
        ))}
      </span>
      <span className="min-w-0 truncate">
        {formatParticipantNames(participants, (count) => t`+${count} more`)}
      </span>
    </div>
  );
}

/**
 * Minutes view: the title is the summary editor's first-line heading, so the
 * meta row can't sit in normal flow below it. It is laid over the gap the
 * column reserves under that heading (`data-cf-meta`, see c-final.css) and
 * follows the heading's height as it wraps or unfolds.
 */
export function TitleAttachedMeta({ sessionId }: { sessionId: string }) {
  const participants = useMetaParticipants(sessionId);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const [top, setTop] = useState<number | null>(null);
  const hasParticipants = participants.length > 0;

  useLayoutEffect(() => {
    const column = anchorRef.current?.closest<HTMLElement>(
      "[data-session-content-column]",
    );
    if (!column || !hasParticipants) {
      setTop(null);
      return;
    }
    column.setAttribute("data-cf-meta", "");

    const measure = () => {
      const heading = column.querySelector<HTMLElement>(
        ".note-title-editor > h1:first-of-type",
      );
      if (!heading) {
        setTop(null);
        return;
      }
      const columnTop = column.getBoundingClientRect().top;
      const headingBottom = heading.getBoundingClientRect().bottom;
      setTop(Math.round(headingBottom - columnTop + 10));
    };
    measure();

    const resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(measure);
    resizeObserver?.observe(column);
    // The editor mounts after this header and swaps its heading node on
    // edits, so watch for DOM changes too.
    const mutationObserver =
      typeof MutationObserver === "undefined"
        ? null
        : new MutationObserver(measure);
    mutationObserver?.observe(column, {
      childList: true,
      subtree: true,
      characterData: true,
      attributes: true,
      attributeFilter: ["class"],
    });
    column.addEventListener("focusin", measure);
    column.addEventListener("focusout", measure);

    return () => {
      column.removeAttribute("data-cf-meta");
      resizeObserver?.disconnect();
      mutationObserver?.disconnect();
      column.removeEventListener("focusin", measure);
      column.removeEventListener("focusout", measure);
    };
  }, [hasParticipants]);

  return (
    <span ref={anchorRef} data-session-title-meta className="contents">
      {hasParticipants && top !== null ? (
        <SessionMetaRow
          participants={participants}
          // Insets match the column's padding (note-input/index.tsx).
          className="pointer-events-none absolute right-5 left-5 z-[1] @3xl:right-8 @3xl:left-8"
          style={{ top }}
        />
      ) : null}
    </span>
  );
}
