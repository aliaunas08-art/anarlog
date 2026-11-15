import { Trans } from "@lingui/react/macro";

import { ListChecks } from "@anlg/ui/components/icons";
import { cn } from "@anlg/utils";

import { useTabs } from "~/store/zustand/tabs";
import { openTasksTab } from "~/task/open-tasks-tab";
import { useOpenTaskCount } from "~/task/tasks-data";

export function SidebarTasksLink() {
  const open = useOpenTaskCount();
  const isActive = useTabs((state) => state.currentTab?.type === "tasks");

  return (
    <button
      type="button"
      onClick={openTasksTab}
      aria-current={isActive ? "page" : undefined}
      className={cn([
        "mx-1 mb-1 flex h-9 shrink-0 items-center justify-between gap-2 rounded-lg px-2",
        "border-t border-[var(--cf-separator)] text-[13px] whitespace-nowrap text-[var(--cf-label)]",
        "cf-motion transition-colors hover:bg-[var(--cf-fill)]",
        isActive && "cf-glass-sel",
      ])}
    >
      <span className="flex min-w-0 items-center gap-2">
        <ListChecks className="size-3.5 shrink-0" aria-hidden />
        <span className="truncate">
          <Trans>Tasks</Trans>
        </span>
      </span>
      {open > 0 ? (
        <span className="text-[var(--cf-secondary)] tabular-nums">
          <Trans>{open} open</Trans>
        </span>
      ) : null}
    </button>
  );
}
