import { useTabs } from "~/store/zustand/tabs";

/** Opens (or focuses) the singleton Tasks database tab. */
export function openTasksTab() {
  useTabs.getState().openNew({ type: "tasks" });
}
