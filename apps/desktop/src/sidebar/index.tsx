import { type CSSProperties, type ReactNode } from "react";

import { cn } from "@anlg/utils";

import { AutomationsNav } from "./automations";
import { CalendarNav } from "./calendar";
import { ContactsNav } from "./contacts";
import { SidebarDateFilter } from "./date-filter";
import { FolderMaterialsPanel } from "./folder-materials";
import { FoldersNav } from "./folders";
import { type SidebarNoteFilter, useSidebarNotes } from "./note-filter";
import { SidebarProjects } from "./projects";
import { SettingsNav } from "./settings";
import { SharedNotesNav } from "./shared-notes";
import { TemplatesNav } from "./templates";
import { TimelineView } from "./timeline";
import { SidebarUpNext } from "./up-next";
import { hasOwnSidebarHeaderTab } from "./use-custom-sidebar";

import { usesTitleBarSidebarActions } from "~/shared/hooks/useWindowControlsGutter";
import { useTabs } from "~/store/zustand/tabs";

export function LeftSidebar({
  folderFilter = null,
  noteFilter = "mine",
  timelineHeader,
  showIgnoredTimelineEvents,
  onShowIgnoredTimelineEventsChange,
}: {
  folderFilter?: string | null;
  noteFilter?: SidebarNoteFilter;
  timelineHeader?: ReactNode;
  showIgnoredTimelineEvents?: boolean;
  onShowIgnoredTimelineEventsChange?: (showIgnored: boolean) => void;
} = {}) {
  const currentTab = useTabs((state) => state.currentTab);
  const showDateFilter = useSidebarNotes((state) => state.showDateFilter);

  const isSettingsMode = currentTab?.type === "settings";
  const isCalendarMode = currentTab?.type === "calendar";
  const isContactsMode = currentTab?.type === "contacts";
  const isTemplatesMode = currentTab?.type === "templates";
  const isAutomationsMode = currentTab?.type === "automations";
  const isFoldersMode = currentTab?.type === "folders";
  const isSpecialMode =
    isSettingsMode ||
    isCalendarMode ||
    isContactsMode ||
    isTemplatesMode ||
    isAutomationsMode ||
    isFoldersMode;
  const isTimelineSidebarLayout = !isSpecialMode;
  // Navs with their own CustomSidebarHeader fill the chrome row themselves; a
  // top padding here would push the header out of it (and overflow-hidden
  // would clip a pulled-up header).
  const needsChromeRowGutter =
    isSpecialMode && !hasOwnSidebarHeaderTab(currentTab);
  const timelineTopChromeInset =
    isTimelineSidebarLayout && !timelineHeader && !usesTitleBarSidebarActions();
  return (
    <div
      data-sidebar-panel-frame
      className="flex h-full w-full shrink-0 flex-col"
    >
      <div
        data-sidebar-panel
        style={SIDEBAR_PANEL_CHROME_INSET}
        className={cn([
          "cf-sidebar-panel flex min-h-0 w-full flex-1 flex-col gap-1 overflow-hidden",
          needsChromeRowGutter ? "pt-9" : "pt-0",
          !isTimelineSidebarLayout && "pr-1",
        ])}
      >
        <div className="flex flex-1 flex-col gap-1 overflow-hidden">
          {isTimelineSidebarLayout ? timelineHeader : null}
          <div className="relative min-h-0 flex-1 overflow-hidden">
            {isSettingsMode ? (
              <SettingsNav />
            ) : isCalendarMode ? (
              <CalendarNav />
            ) : isContactsMode ? (
              <ContactsNav />
            ) : isTemplatesMode ? (
              <TemplatesNav />
            ) : isAutomationsMode ? (
              <AutomationsNav />
            ) : isFoldersMode ? (
              <FoldersNav />
            ) : (
              <div className="flex h-full min-h-0 flex-col">
                {noteFilter === "mine" ? (
                  <>
                    {folderFilter ? (
                      <FolderMaterialsPanel folderPath={folderFilter} />
                    ) : null}
                    {/* Clear the title-bar chrome that would otherwise
                     * overlay the top of the sidebar. */}
                    <SidebarProjects
                      className={timelineTopChromeInset ? "pt-12" : "pt-3"}
                    />
                    {showDateFilter ? (
                      <SidebarDateFilter
                        folderFilter={folderFilter}
                        className="pt-3"
                      />
                    ) : null}
                    <SidebarUpNext />
                    <div className="relative min-h-0 flex-1">
                      <TimelineView
                        folderFilter={folderFilter}
                        showIgnoredEvents={showIgnoredTimelineEvents}
                        onShowIgnoredEventsChange={
                          onShowIgnoredTimelineEventsChange
                        }
                        topChipsOverlapHeader={
                          isTimelineSidebarLayout && !!timelineHeader
                        }
                      />
                    </div>
                  </>
                ) : (
                  <SharedNotesNav />
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// The flat panel starts at the window's top-left corner, so rows aligned to
// the traffic lights need no inset correction.
const SIDEBAR_PANEL_CHROME_INSET = {
  "--cf-chrome-inset-x": "0px",
  "--cf-chrome-inset-y": "0px",
} as CSSProperties;
