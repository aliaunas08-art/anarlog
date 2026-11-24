import { useLingui } from "@lingui/react/macro";
import { memo, type ReactNode } from "react";

import {
  CalendarBlank,
  House,
  ListChecks,
  MagnifyingGlass,
  NotePencil,
  Plus,
  Sidebar,
  SidebarSimple,
} from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { openHome } from "~/shared/leave-overlay-tab";
import { SidebarNoteFilterMenu } from "~/sidebar/note-filter-menu";
import { useSidebarUpcomingMeetingStatus } from "~/sidebar/timeline/upcoming-meeting";
import { useTabs } from "~/store/zustand/tabs";
import { openTasksTab } from "~/task/open-tasks-tab";
import { useOpenTaskCount } from "~/task/tasks-data";

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
              <SidebarSimple size={14} />
            ) : (
              <Sidebar size={14} />
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

// Airy nav rows under the traffic-light row (Claude / ChatGPT sidebars):
// plain icon + label rows with a hover/selected fill only.
export function SidebarPrimaryActions({
  onNewNote,
  onSearch,
}: {
  onNewNote: () => void;
  onSearch: () => void;
}) {
  const { t } = useLingui();
  const openTasks = useOpenTaskCount();
  const currentTabType = useTabs((state) => state.currentTab?.type);

  return (
    <nav
      data-sidebar-primary-actions
      aria-label={t`Sidebar`}
      className="flex shrink-0 flex-col gap-px px-2 pt-1 pb-1"
    >
      <SidebarNavRow
        label={t`Home`}
        icon={<House size={16} aria-hidden />}
        active={currentTabType === "empty"}
        onClick={openHome}
      />
      <SidebarNavRow
        label={t`New note`}
        icon={<Plus size={16} aria-hidden />}
        hint="⌘N"
        onClick={onNewNote}
      />
      <SidebarNavRow
        label={t`Search`}
        icon={<MagnifyingGlass size={16} aria-hidden />}
        hint="⌘K"
        onClick={onSearch}
      />
      <SidebarNavRow
        label={t`Tasks`}
        icon={<ListChecks size={16} aria-hidden />}
        active={currentTabType === "tasks"}
        trailing={openTasks > 0 ? String(openTasks) : null}
        trailingLabel={openTasks > 0 ? t`${openTasks} open` : undefined}
        onClick={openTasksTab}
      />
      <SidebarNavRow
        label={t`Calendar`}
        icon={<CalendarBlank size={16} aria-hidden />}
        active={currentTabType === "calendar"}
        onClick={() => useTabs.getState().openNew({ type: "calendar" })}
      />
    </nav>
  );
}

function SidebarNavRow({
  label,
  icon,
  hint,
  trailing = null,
  trailingLabel,
  active = false,
  onClick,
}: {
  label: string;
  icon: ReactNode;
  hint?: string;
  trailing?: string | null;
  trailingLabel?: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={cn([
        "group/nav-row flex h-9 w-full min-w-0 items-center gap-2.5 rounded-lg px-2.5",
        "text-[15px] whitespace-nowrap text-[var(--cf-label)]",
        "cf-motion transition-colors hover:bg-[var(--cf-selection)]",
        "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-hidden",
        active && "bg-[var(--cf-selection)]",
      ])}
    >
      <span className="flex size-4 shrink-0 items-center justify-center text-[var(--cf-label)] opacity-85">
        {icon}
      </span>
      <span className="min-w-0 truncate">{label}</span>
      {trailing ? (
        <span
          aria-label={trailingLabel}
          className="ml-auto shrink-0 text-[13px] text-[var(--cf-secondary)] tabular-nums"
        >
          {trailing}
        </span>
      ) : hint ? (
        <span className="ml-auto shrink-0 text-[12px] text-[var(--cf-tertiary)] opacity-0 transition-opacity group-hover/nav-row:opacity-100 group-focus-visible/nav-row:opacity-100">
          {hint}
        </span>
      ) : null}
    </button>
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
        "pointer-events-auto relative flex size-7 items-center justify-center rounded-md",
        "cf-motion text-[var(--cf-secondary)] transition-colors hover:bg-[var(--cf-fill)] hover:text-[var(--cf-label)]",
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
