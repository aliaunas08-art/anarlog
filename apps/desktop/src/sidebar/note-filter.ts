import { create } from "zustand";

export type SidebarNoteFilter = "mine" | "shared";
export type SidebarNotesGroupBy = "date" | "folder";
type SidebarNotesSortOrder = "newest" | "oldest";

type SidebarNotesState = {
  noteFilter: SidebarNoteFilter;
  folderFilter: string | null;
  groupBy: SidebarNotesGroupBy;
  sortOrder: SidebarNotesSortOrder;
  /** Local day ("YYYY-MM-DD", configured timezone) the timeline is limited to. */
  dateFilter: string | null;
  showDateFilter: boolean;
  setView: (
    noteFilter: SidebarNoteFilter,
    folderFilter?: string | null,
  ) => void;
  setGroupBy: (groupBy: SidebarNotesGroupBy) => void;
  setSortOrder: (sortOrder: SidebarNotesSortOrder) => void;
  setDateFilter: (dateFilter: string | null) => void;
  setShowDateFilter: (showDateFilter: boolean) => void;
};

const defaultSidebarNotes = {
  noteFilter: "mine" as const,
  folderFilter: null,
  groupBy: "date" as const,
  sortOrder: "newest" as const,
  dateFilter: null,
  // Board: the "By date" calendar is part of the default sidebar.
  showDateFilter: true,
};

export const useSidebarNotes = create<SidebarNotesState>((set) => ({
  ...defaultSidebarNotes,
  setView: (noteFilter, folderFilter) =>
    set({
      noteFilter,
      folderFilter: noteFilter === "shared" ? null : (folderFilter ?? null),
    }),
  setGroupBy: (groupBy) => set({ groupBy }),
  setSortOrder: (sortOrder) => set({ sortOrder }),
  setDateFilter: (dateFilter) =>
    set(
      dateFilter === null
        ? { dateFilter }
        : { dateFilter, showDateFilter: true },
    ),
  // Hiding the calendar also clears its filter so no invisible filter remains.
  setShowDateFilter: (showDateFilter) =>
    set(
      showDateFilter
        ? { showDateFilter }
        : { showDateFilter, dateFilter: null },
    ),
}));

export function resetSidebarNotes() {
  useSidebarNotes.setState({ ...defaultSidebarNotes });
}

export function folderIdForNewNote(
  noteFilter: SidebarNoteFilter,
  folderFilter: string | null,
): string | undefined {
  if (noteFilter !== "mine" || folderFilter === null) {
    return undefined;
  }

  return folderFilter;
}

export function encodeNotesView(
  noteFilter: SidebarNoteFilter,
  folderFilter: string | null,
): string {
  if (noteFilter === "shared") {
    return "shared";
  }

  if (folderFilter !== null) {
    return `folder:${folderFilter}`;
  }

  return "mine";
}

export function decodeNotesView(value: string): {
  noteFilter: SidebarNoteFilter;
  folderFilter: string | null;
} {
  if (value === "shared") {
    return { noteFilter: "shared", folderFilter: null };
  }

  if (value.startsWith("folder:")) {
    return { noteFilter: "mine", folderFilter: value.slice("folder:".length) };
  }

  return { noteFilter: "mine", folderFilter: null };
}
