import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { DiskEncryptionRow } from "~/data-protection/disk-encryption-row";
import { PROVIDERS } from "~/settings/ai/stt/shared";
import { SettingsPageTitle } from "~/settings/page-title";
import { useStoredSettingValuesQuery } from "~/settings/queries";
import { resolveConfigValue } from "~/shared/config";
import { commands as desktopCommands } from "~/types/tauri.gen";

// ASPC D4.2 / D6.3: "Where your data lives". The paths come from Rust (the
// same places the app opens) and the "leaves this Mac" lines from the same
// settings the app uses, so the screen can't drift from them.

export type DataFlowState = {
  sttProviderId: string | undefined;
  analyticsOn: boolean;
  crashReportsOn: boolean;
  sessionReplayOn: boolean;
};

export function sttStaysOnDevice(providerId: string | undefined): boolean {
  if (!providerId) return true;
  const provider = PROVIDERS.find((candidate) => candidate.id === providerId);
  return provider?.badge === "On device";
}

export function sttProviderName(providerId: string | undefined): string {
  return (
    PROVIDERS.find((candidate) => candidate.id === providerId)?.displayName ??
    providerId ??
    ""
  );
}

export function SettingsDataLocations() {
  const { t } = useLingui();
  const settingsQuery = useStoredSettingValuesQuery();
  const locations = useQuery({
    queryKey: ["data-protection", "locations"],
    queryFn: async () => {
      const result = await desktopCommands.getDataLocations();
      if (result.status === "error") throw new Error(result.error);
      return result.data;
    },
  });

  if (!settingsQuery.data) return null;
  const state: DataFlowState = {
    sttProviderId: settingsQuery.data.values.current_stt_provider,
    analyticsOn: resolveConfigValue("analytics_opt_in", settingsQuery.data),
    crashReportsOn: resolveConfigValue(
      "crash_reporting_consent",
      settingsQuery.data,
    ),
    sessionReplayOn: resolveConfigValue(
      "session_replay_opt_in",
      settingsQuery.data,
    ),
  };
  const paths = locations.data;
  const sttName = sttProviderName(state.sttProviderId);

  return (
    <div className="flex flex-col gap-8">
      <SettingsPageTitle title={t`Where your data lives`} />

      <section className="flex flex-col gap-3">
        <SectionTitle>
          <Trans>On this Mac</Trans>
        </SectionTitle>
        <p className="text-muted-foreground text-xs">
          <Trans>
            Your meetings are stored only on this Mac unless you turn on sync.
            Transcription can run on this Mac, depending on the model you
            choose.
          </Trans>
        </p>
        <PathRow
          label={t`Meetings: audio, notes and attachments`}
          path={paths?.sessions}
        />
        <PathRow
          label={t`Database: notes, transcripts and settings`}
          path={paths?.database}
        />
        <PathRow label={t`Search index`} path={paths?.search_index} />
        <PathRow
          label={t`Audit log (ids and counts only)`}
          path={paths?.audit_log}
        />
        <PathRow label={t`App logs (no meeting content)`} path={paths?.logs} />
        <PathRow
          label={t`Keys and tokens`}
          path={t`macOS Keychain (never in files or the database)`}
        />
        <DiskEncryptionRow />
      </section>

      <section className="flex flex-col gap-3">
        <SectionTitle>
          <Trans>What leaves this Mac</Trans>
        </SectionTitle>
        <FlowRow title={t`Anthropic (AI)`}>
          <Trans>
            When you enhance a meeting or use Ask, the meeting's text
            (transcript, notes, title) is sent to Anthropic's API with your own
            key, through the app's Efficiency Engine, which fixes the model
            (Claude Haiku). Nothing is sent until you add a key in Settings →
            AI.
          </Trans>
        </FlowRow>
        <FlowRow title={t`Transcription`}>
          {sttStaysOnDevice(state.sttProviderId) ? (
            <Trans>
              Your current speech-to-text model runs on this Mac: audio stays
              here.
            </Trans>
          ) : (
            <Trans>
              While transcribing, meeting audio is sent to {sttName}, the
              speech-to-text provider you chose.
            </Trans>
          )}
        </FlowRow>
        <FlowRow title={t`Calendar`}>
          <Trans>
            If you connect a calendar, the app reads your events (read-only)
            directly from Google or Microsoft. Nothing is written back.
          </Trans>
        </FlowRow>
        <FlowRow title={t`Sync`}>
          <Trans>
            Only if you sign in and turn on sync in Settings → Sync: an
            end-to-end encrypted copy of your library is kept on the sync server
            so your other devices can read it.
          </Trans>
        </FlowRow>
        <FlowRow title={t`Updates`}>
          <Trans>A version check. No library data is sent.</Trans>
        </FlowRow>
        <FlowRow title={t`Usage data`}>
          {state.analyticsOn ? (
            <Trans>
              On: event names, app version and counts. Never meeting content.
            </Trans>
          ) : (
            <Trans>Off. Nothing is sent.</Trans>
          )}
        </FlowRow>
        <FlowRow title={t`Error reports`}>
          {state.crashReportsOn ? (
            state.sessionReplayOn ? (
              <Trans>
                On, with session replay (all text and inputs masked). Never
                meeting content.
              </Trans>
            ) : (
              <Trans>On: sanitized error details. Never meeting content.</Trans>
            )
          ) : (
            <Trans>Off. Nothing is sent.</Trans>
          )}
        </FlowRow>
      </section>

      <section className="flex flex-col gap-2">
        <SectionTitle>
          <Trans>Never</Trans>
        </SectionTitle>
        <p className="text-sm">
          <Trans>
            Meeting content (audio, transcripts, notes, titles and participants)
            is never used for analytics, telemetry, product research or model
            training.
          </Trans>
        </p>
        <p className="text-muted-foreground text-xs">
          <Trans>
            Anthropic's commercial API terms say API inputs and outputs are not
            used to train its models by default.
          </Trans>
        </p>
      </section>
    </div>
  );
}

function SectionTitle({ children }: { children: ReactNode }) {
  return <h3 className="text-sm font-semibold">{children}</h3>;
}

function PathRow({ label, path }: { label: string; path?: string | null }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-sm font-medium">{label}</span>
      <span className="text-muted-foreground font-mono text-xs break-all select-text">
        {path ?? "…"}
      </span>
    </div>
  );
}

function FlowRow({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-sm font-medium">{title}</span>
      <p className="text-muted-foreground text-xs">{children}</p>
    </div>
  );
}
