import { Trans, useLingui } from "@lingui/react/macro";
import { useInfiniteQuery, useMutation } from "@tanstack/react-query";

import {
  commands as auditCommands,
  type Entry,
  type VerifyReport,
} from "@anlg/plugin-audit";
import { Button } from "@anlg/ui/components/ui/button";

import { SettingsPageTitle } from "~/settings/page-title";

const PAGE_SIZE = 50;

// Settings → Audit (ASPC D1.6). Entries come from the separate audit.db in
// Rust; this page can only read them and run Verify.
export function SettingsAudit() {
  const { t } = useLingui();

  return (
    <div className="flex flex-col gap-8">
      <SettingsPageTitle title={t`Audit`} />
      <p className="text-muted-foreground text-sm">
        <Trans>
          Sensitive actions are recorded here: unlocking the app, opening,
          exporting or deleting meetings, Ask queries, settings and key changes,
          and denied requests. Entries hold ids and counts only, never meeting
          content. The log is append-only and hash-chained.
        </Trans>
      </p>
      <VerifySection />
      <EntriesSection />
    </div>
  );
}

function VerifySection() {
  const verify = useMutation({
    mutationFn: async () => {
      const result = await auditCommands.verify();
      if (result.status === "error") throw new Error(result.error);
      return result.data;
    },
  });

  return (
    <section className="flex flex-col gap-2">
      <div>
        <Button
          variant="outline"
          size="sm"
          disabled={verify.isPending}
          onClick={() => verify.mutate()}
        >
          <Trans>Verify</Trans>
        </Button>
      </div>
      {verify.data ? <VerifyResult report={verify.data} /> : null}
      {verify.error ? (
        <p role="alert" className="text-destructive text-sm">
          {verify.error.message}
        </p>
      ) : null}
    </section>
  );
}

export function VerifyResult({ report }: { report: VerifyReport }) {
  return (
    <div
      role="status"
      data-audit-verify
      className="flex flex-col gap-1 text-sm"
    >
      {report.ok ? (
        <p>
          <Trans>OK: {report.entries} entries, chain intact.</Trans>
        </p>
      ) : (
        <p className="text-destructive">
          <Trans>
            Tampering detected at entry {report.first_bad_seq ?? "?"}:{" "}
            {report.reason ?? ""}
          </Trans>
        </p>
      )}
      {report.head_note ? (
        <p className="text-muted-foreground text-xs">{report.head_note}</p>
      ) : null}
    </div>
  );
}

function EntriesSection() {
  const entries = useInfiniteQuery({
    queryKey: ["audit", "entries"],
    initialPageParam: null as number | null,
    queryFn: async ({ pageParam }) => {
      const result = await auditCommands.listEntries(pageParam, PAGE_SIZE);
      if (result.status === "error") throw new Error(result.error);
      return result.data;
    },
    getNextPageParam: (page) => page.next_before_seq ?? undefined,
  });

  if (entries.error) {
    return (
      <p role="alert" className="text-destructive text-sm">
        {entries.error.message}
      </p>
    );
  }
  const rows = entries.data?.pages.flatMap((page) => page.entries) ?? [];
  const total = entries.data?.pages[0]?.total ?? 0;

  return (
    <section className="flex flex-col gap-2">
      <p className="text-muted-foreground text-xs">
        <Trans>{total} entries</Trans>
      </p>
      <div className="overflow-x-auto">
        <table className="w-full text-left text-xs">
          <thead className="text-muted-foreground">
            <tr>
              <th className="py-1 pr-3">#</th>
              <th className="py-1 pr-3">
                <Trans>Time</Trans>
              </th>
              <th className="py-1 pr-3">
                <Trans>Action</Trans>
              </th>
              <th className="py-1 pr-3">
                <Trans>Object</Trans>
              </th>
              <th className="py-1 pr-3">
                <Trans>Source</Trans>
              </th>
              <th className="py-1">
                <Trans>Details</Trans>
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((entry) => (
              <EntryRow key={entry.seq} entry={entry} />
            ))}
          </tbody>
        </table>
      </div>
      {entries.hasNextPage ? (
        <div>
          <Button
            variant="ghost"
            size="sm"
            disabled={entries.isFetchingNextPage}
            onClick={() => void entries.fetchNextPage()}
          >
            <Trans>Load older entries</Trans>
          </Button>
        </div>
      ) : null}
    </section>
  );
}

function EntryRow({ entry }: { entry: Entry }) {
  return (
    <tr className="border-border border-t align-top">
      <td className="py-1 pr-3 tabular-nums">{entry.seq}</td>
      <td className="py-1 pr-3 whitespace-nowrap">
        {new Date(entry.ts).toLocaleString()}
      </td>
      <td className="py-1 pr-3 font-mono">{entry.action}</td>
      <td className="py-1 pr-3 font-mono break-all">
        {entry.object_type}
        {entry.object_id ? `:${entry.object_id}` : ""}
      </td>
      <td className="py-1 pr-3">{entry.source}</td>
      <td className="text-muted-foreground py-1 font-mono break-all">
        {entry.detail === "{}" ? "" : entry.detail}
      </td>
    </tr>
  );
}
