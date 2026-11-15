import { Trans } from "@lingui/react/macro";
import { memo, type ReactNode } from "react";

import {
  MagnifyingGlass,
  NotePencil,
  Plus,
  Sidebar,
  SidebarSimple,
} from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { SidebarNoteFilterMenu } from "~/sidebar/note-filter-menu";
import { useSidebarUpcomingMeetingStatus } from "~/sidebar/timeline/upcoming-meeting";

export const SidebarTimelineChromeWithUpcomingMeeting = memo(
  function SidebarTimelineChromeWithUpcomingMeeting({
    currentSessionId,
    onToggleSidebar,
    sidebarExpanded,
    showSidebarToggle = true,
    showIgnoredTimelineEvents,
  }: {
    currentSessionId?: string;
    onToggleSidebar: () => void;
    sidebarExpanded: boolean;
    showSidebarToggle?: boolean;
    showIgnoredTimelineEvents: boolean;
  }) {
    const upcomingMeetingStatus = useSidebarUpcomingMeetingStatus({
      showIgnored: showIgnoredTimelineEvents,
    });
    const hasUpcomingMeeting = upcomingMeetingStatus
      ? !currentSessionId ||
        upcomingMeetingStatus.itemKey !== `session-${currentSessionId}`
      : false;

    return (
      <SidebarTimelineChrome
        hasUpcomingMeeting={hasUpcomingMeeting}
        onToggleSidebar={onToggleSidebar}
        sidebarExpanded={sidebarExpanded}
        showSidebarToggle={showSidebarToggle}
      />
    );
  },
);

function SidebarTimelineChrome({
  hasUpcomingMeeting,
  onToggleSidebar,
  sidebarExpanded,
  showSidebarToggle,
}: {
  hasUpcomingMeeting: boolean;
  onToggleSidebar: () => void;
  sidebarExpanded: boolean;
  showSidebarToggle: boolean;
}) {
  const collapsedBadge = !sidebarExpanded
    ? hasUpcomingMeeting
      ? "upcomingMeeting"
      : null
    : null;

  return (
    <div data-tauri-drag-region className="flex w-full items-center">
      <div data-tauri-drag-region className="flex items-center gap-0">
        {showSidebarToggle && (
          <LeftSurfaceChromeButton
            ariaLabel={sidebarExpanded ? "Hide sidebar" : "Show sidebar"}
            badge={collapsedBadge}
            onClick={onToggleSidebar}
          >
            {sidebarExpanded ? (
              <SidebarSimple size={16} />
            ) : (
              <Sidebar size={16} />
            )}
          </LeftSurfaceChromeButton>
        )}
      </div>
      {sidebarExpanded ? (
        <div data-tauri-drag-region className="ml-auto flex items-center">
          <SidebarNoteFilterMenu />
        </div>
      ) : null}
    </div>
  );
}

// Board: a prominent "New note ⌘N" capsule and a search field under the
// traffic-light row. Only rendered on macOS, where the shortcuts are ⌘.
export function SidebarPrimaryActions({
  onNewNote,
  onSearch,
}: {
  onNewNote: () => void;
  onSearch: () => void;
}) {
  return (
    <div
      data-sidebar-primary-actions
      className="flex shrink-0 flex-col gap-2 px-2.5 pt-1 pb-2"
    >
      <button
        type="button"
        aria-label="New note"
        onClick={onNewNote}
        className={cn([
          "cf-glass-tint rounded-pill flex h-8 w-full min-w-0 items-center gap-2 px-2.5",
          "text-[13px] font-semibold whitespace-nowrap",
          "cf-motion transition-[filter] hover:brightness-110 active:brightness-95",
          "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:ring-offset-2 focus-visible:outline-hidden",
        ])}
      >
        <Plus size={13} weight="bold" aria-hidden className="shrink-0" />
        <span className="min-w-0 truncate">
          <Trans>New note</Trans>
        </span>
        <span className="ml-auto shrink-0 text-[11px] font-medium opacity-75">
          ⌘N
        </span>
      </button>
      <button
        type="button"
        aria-label="Search"
        onClick={onSearch}
        className={cn([
          "flex h-[30px] w-full min-w-0 items-center gap-1.5 rounded-lg px-2",
          "bg-[var(--cf-fill)] text-[13px] whitespace-nowrap text-[var(--cf-secondary)]",
          "cf-motion transition-colors hover:text-[var(--cf-label)]",
          "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-hidden",
        ])}
      >
        <MagnifyingGlass size={13} aria-hidden className="shrink-0" />
        <span className="min-w-0 truncate">
          <Trans>Search</Trans>
        </span>
        <span className="ml-auto shrink-0 text-[11px]">⌘P</span>
      </button>
    </div>
  );
}

export function SidebarNoteActions({
  onNewNote,
  onSearch,
}: {
  onNewNote: () => void;
  onSearch: () => void;
}) {
  return (
    <>
      <LeftSurfaceChromeButton ariaLabel="Search" onClick={onSearch}>
        <MagnifyingGlass size={15} />
      </LeftSurfaceChromeButton>
      <LeftSurfaceChromeButton ariaLabel="New note" onClick={onNewNote}>
        <NotePencil size={15} />
      </LeftSurfaceChromeButton>
      <SidebarNoteFilterMenu />
    </>
  );
}

export function LeftSurfaceChromeButton({
  ariaLabel,
  badge = null,
  children,
  disabled = false,
  onClick,
}: {
  ariaLabel: string;
  badge?: "upcomingMeeting" | null;
  children: ReactNode;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={ariaLabel}
      data-tauri-drag-region="false"
      disabled={disabled}
      className={cn([
        "pointer-events-auto relative flex size-7 items-center justify-center rounded-full",
        "text-muted-foreground hover:bg-accent hover:text-foreground transition-colors",
        "focus-visible:ring-ring focus-visible:ring-2 focus-visible:outline-hidden",
        "disabled:text-muted-foreground/70 disabled:hover:text-muted-foreground/70 disabled:hover:bg-transparent",
      ])}
      onClick={onClick}
    >
      {children}
      {badge ? (
        <span
          aria-hidden="true"
          data-testid="collapsed-sidebar-upcoming-meeting-badge"
          className="ring-background pointer-events-none absolute top-1 right-1 size-1.5 rounded-full bg-red-500 ring-2"
        />
      ) : null}
    </button>
  );
}
