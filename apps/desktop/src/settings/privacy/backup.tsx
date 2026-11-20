import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";

import { commands as fs2Commands } from "@anlg/plugin-fs2";
import { commands as sessionCommands } from "@anlg/plugin-session";
import { Button } from "@anlg/ui/components/ui/button";
import { toast } from "@anlg/ui/components/ui/toast";

import { scheduleAutomaticRelaunch } from "~/shared/relaunch";
import { listenerStore } from "~/store/zustand/listener/instance";

const BACKUP_EXTENSION = "anarlogbackup";
const CANCELLED = "backup.cancelled";

function backupFileName(now = new Date()): string {
  const day = now.toISOString().slice(0, 10);
  return `Anarlog Backup ${day}.${BACKUP_EXTENSION}`;
}

function isCancelled(error: unknown): boolean {
  return error instanceof Error && error.message.includes(CANCELLED);
}

// ASPC C5: "Back up library" and "Restore from backup". The paths come from
// native dialogs opened by Rust (A3.6), and Rust asks for the confirmations
// (unencrypted backup; replacing the library), so the webview can't skip
// them. A restore finishes on the next launch.
export function BackupSettings() {
  const { t } = useLingui();
  const filters = [{ name: t`Anarlog backup`, extensions: [BACKUP_EXTENSION] }];

  const backup = useMutation({
    mutationFn: async () => {
      const picked = await fs2Commands.pickSavePath(backupFileName(), filters);
      if (picked.status === "error") throw new Error(picked.error);
      if (!picked.data) return null;
      const result = await sessionCommands.backupLibrary(picked.data);
      if (result.status === "error") throw new Error(result.error);
      return result.data;
    },
    onSuccess: (summary) => {
      if (!summary) return;
      toast.success(t`Library backed up`, {
        description: t`${summary.meetings} meetings and ${summary.files} files.`,
      });
    },
    onError: (error) => {
      if (isCancelled(error)) return;
      toast.error(t`Backup failed`, { description: error.message });
    },
  });

  const restore = useMutation({
    mutationFn: async () => {
      const picked = await fs2Commands.pickOpenPaths(
        t`Choose a backup to restore`,
        false,
        filters,
      );
      if (picked.status === "error") throw new Error(picked.error);
      const path = picked.data[0];
      if (!path) return null;
      const result = await sessionCommands.restoreLibrary(path);
      if (result.status === "error") throw new Error(result.error);
      return result.data;
    },
    onSuccess: (summary) => {
      if (!summary) return;
      toast.success(t`Restore ready`, {
        duration: Infinity,
        description: t`${summary.meetings} meetings will be restored when the app restarts. Your current library is kept in the app's data folder.`,
        action: {
          label: t`Restart now`,
          onClick: () => {
            if (listenerStore.getState().live.status === "active") {
              toast.error(t`Stop the recording first`, {
                description: t`The restore finishes when the app restarts.`,
              });
              return;
            }
            void scheduleAutomaticRelaunch();
          },
        },
      });
    },
    onError: (error) => {
      if (isCancelled(error)) return;
      toast.error(t`Restore failed`, { description: error.message });
    },
  });

  return (
    <section className="flex flex-col gap-3">
      <div>
        <h3 className="mb-1 text-sm font-medium">
          <Trans>Backup</Trans>
        </h3>
        <p className="text-muted-foreground text-xs">
          <Trans>
            A backup holds every meeting, transcript, note and recording in one
            file. It is not encrypted, so keep it somewhere only you can reach.
            The audit log and your keys are not included.
          </Trans>
        </p>
      </div>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={backup.isPending || restore.isPending}
          onClick={() => backup.mutate()}
        >
          <Trans>Back up library…</Trans>
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={backup.isPending || restore.isPending}
          onClick={() => restore.mutate()}
        >
          <Trans>Restore from backup…</Trans>
        </Button>
      </div>
    </section>
  );
}
