import { Trans, useLingui } from "@lingui/react/macro";
import { useId, useMemo, useState } from "react";

import { CaretDown, FolderSimple, Plus } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { FolderNameDialog } from "./folder-name-dialog";

import { useTimelineSessionsTable } from "~/calendar/queries";
import { useActiveFolderPath, useFolderSelection } from "~/folders/selection";
import { createNamedFolder } from "~/session/folder-catalog";
import { folderPathMatchesFilter } from "~/session/folders";
import { useFolderPaths } from "~/session/queries";
import { useTabs } from "~/store/zustand/tabs";

/** Sidebar "Projects": the user's folders (named Projects in the UI). */
export function SidebarProjects({ className }: { className?: string }) {
  const { t } = useLingui();
  const folders = useFolderPaths();
  const sessions = useTimelineSessionsTable();
  const activeFolder = useActiveFolderPath(folders);
  const isFoldersTab = useTabs((state) => state.currentTab?.type === "folders");
  const [expanded, setExpanded] = useState(true);
  const [creating, setCreating] = useState(false);
  const listId = useId();

  const counts = useMemo(() => {
    const rows = Object.values(sessions ?? {});
    return new Map(
      folders.map((folder) => [
        folder,
        rows.filter((row) => folderPathMatchesFilter(row.folder_id, folder))
          .length,
      ]),
    );
  }, [folders, sessions]);

  const openProject = (folder: string) => {
    useFolderSelection.getState().setSelectedPath(folder);
    useTabs.getState().openNew({ type: "folders" });
  };

  return (
    <section
      aria-label={t`Projects`}
      data-sidebar-projects
      className={cn(["flex shrink-0 flex-col px-2 pb-1", className])}
    >
      <div className="group/projects flex h-8 items-center gap-1 pr-0.5 pl-2.5">
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={listId}
          onClick={() => setExpanded((value) => !value)}
          className={cn([
            "flex min-w-0 flex-1 items-center gap-1.5 rounded-md text-left",
            "text-[13px] leading-[18px] font-medium text-[var(--cf-secondary)]",
            "cf-motion transition-colors hover:text-[var(--cf-label)]",
            "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-hidden",
          ])}
        >
          <span className="truncate">
            <Trans>Projects</Trans>
          </span>
          <CaretDown
            size={12}
            aria-hidden
            className={cn([
              "cf-motion shrink-0 transition-transform",
              !expanded && "-rotate-90",
            ])}
          />
        </button>
        <button
          type="button"
          aria-label={t`New project`}
          title={t`New project`}
          onClick={() => setCreating(true)}
          className={cn([
            "rounded-pill inline-flex size-7 shrink-0 items-center justify-center",
            "cf-motion text-[var(--cf-secondary)] transition-colors hover:bg-[var(--cf-fill)] hover:text-[var(--cf-label)]",
            "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-hidden",
          ])}
        >
          <Plus size={14} aria-hidden />
        </button>
      </div>

      {expanded ? (
        <ul
          id={listId}
          className="scrollbar-hide flex max-h-[180px] flex-col gap-px overflow-y-auto"
        >
          {folders.length === 0 ? (
            <li className="flex h-9 items-center px-2.5 text-[14px] text-[var(--cf-tertiary)]">
              <Trans>No projects yet</Trans>
            </li>
          ) : (
            folders.map((folder) => {
              const count = counts.get(folder) ?? 0;
              const active = isFoldersTab && folder === activeFolder;
              return (
                <li key={folder}>
                  <button
                    type="button"
                    aria-current={active ? "page" : undefined}
                    onClick={() => openProject(folder)}
                    className={cn([
                      "flex h-9 w-full min-w-0 items-center gap-2.5 rounded-lg px-2.5 text-left",
                      "text-[15px] whitespace-nowrap text-[var(--cf-label)]",
                      "cf-motion transition-colors hover:bg-[var(--cf-selection)]",
                      "focus-visible:ring-2 focus-visible:ring-[var(--cf-accent)] focus-visible:outline-hidden",
                      active && "bg-[var(--cf-selection)]",
                    ])}
                  >
                    <span className="flex size-4 shrink-0 items-center justify-center opacity-85">
                      <FolderSimple size={16} aria-hidden />
                    </span>
                    <span className="min-w-0 truncate">{folder}</span>
                    {count > 0 ? (
                      <span
                        aria-label={count === 1 ? t`1 note` : t`${count} notes`}
                        className="ml-auto shrink-0 text-[13px] text-[var(--cf-secondary)] tabular-nums"
                      >
                        {count}
                      </span>
                    ) : null}
                  </button>
                </li>
              );
            })
          )}
        </ul>
      ) : null}

      <FolderNameDialog
        open={creating}
        title={t`New project`}
        confirmLabel={t`Create`}
        onOpenChange={setCreating}
        onSubmit={async (path) => {
          const created = await createNamedFolder(path);
          openProject(created);
        }}
      />
    </section>
  );
}
