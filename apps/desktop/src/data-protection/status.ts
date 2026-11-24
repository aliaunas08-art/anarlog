import { useQuery } from "@tanstack/react-query";

import { commands as desktopCommands } from "~/types/tauri.gen";

export type FileVaultState = "on" | "off" | "unknown";

function toFileVaultState(value: string): FileVaultState {
  return value === "on" || value === "off" ? value : "unknown";
}

export async function getFileVaultState(): Promise<FileVaultState> {
  try {
    const status = await desktopCommands.getDataProtectionStatus();
    return toFileVaultState(status.filevault);
  } catch {
    return "unknown";
  }
}

// ASPC C2.1: the database is not encrypted by the app, so the app checks that
// FileVault is on (Rust runs `fdesetup status`).
export function useFileVaultState() {
  return useQuery({
    queryKey: ["data-protection", "filevault"],
    queryFn: getFileVaultState,
    staleTime: 5 * 60_000,
  });
}
