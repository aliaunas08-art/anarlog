import { Trans, useLingui } from "@lingui/react/macro";
import { useForm } from "@tanstack/react-form";

import type { EngineStatus, Quotas, UsageCounts } from "@anlg/plugin-ai-engine";
import { Button } from "@anlg/ui/components/ui/button";
import { Input } from "@anlg/ui/components/ui/input";

import {
  pauseMessage,
  useClearEngineKey,
  useEngineKeyStatus,
  useEngineStatus,
  useSetEngineKey,
  useSetEngineQuotas,
} from "~/ai/engine";
import { SettingsPageTitle } from "~/settings/page-title";

// Settings → AI. The Efficiency Engine (ASPC C7) replaces the provider and
// model picker: one locked model, a key kept in the Keychain, local quotas.
export function AiEngineSettings() {
  const { t } = useLingui();
  const status = useEngineStatus();

  return (
    <div className="flex flex-col gap-8">
      <SettingsPageTitle title={t`AI`} />
      <EngineStatusSection status={status.data} />
      <ApiKeySection />
      {status.data ? <UsageSection status={status.data} /> : null}
      {status.data ? <QuotasSection quotas={status.data.quotas} /> : null}
    </div>
  );
}

function EngineStatusSection({ status }: { status: EngineStatus | undefined }) {
  const keyStatus = useEngineKeyStatus();
  const present = keyStatus.data?.present;

  return (
    <section data-ai-engine-status className="flex flex-col gap-2 text-sm">
      <div className="flex flex-wrap gap-x-6 gap-y-1">
        <span>
          <Trans>Mode:</Trans>{" "}
          <strong>
            {status?.mode === "proxy" ? (
              <Trans>Proxy</Trans>
            ) : (
              <Trans>Direct</Trans>
            )}
          </strong>
        </span>
        <span>
          <Trans>Model:</Trans> <strong>{status?.model ?? "…"}</strong>
        </span>
        <span>
          <Trans>Key:</Trans>{" "}
          <strong>
            {present === undefined ? (
              "…"
            ) : present ? (
              <Trans>saved in Keychain</Trans>
            ) : (
              <Trans>missing</Trans>
            )}
          </strong>
        </span>
      </div>
      {status?.pause ? (
        <p role="status" className="text-muted-foreground">
          {pauseMessage(status.pause.code, status.pause.message)}
        </p>
      ) : null}
      <p className="text-muted-foreground text-xs">
        <Trans>
          Every AI feature uses this one model. Meeting text is sent to the AI
          provider only when you enhance a note or ask a question.
        </Trans>
      </p>
    </section>
  );
}

function ApiKeySection() {
  const { t } = useLingui();
  const setKey = useSetEngineKey();
  const clearKey = useClearEngineKey();
  const keyStatus = useEngineKeyStatus();
  const form = useForm({
    defaultValues: { key: "" },
    onSubmit: async ({ value, formApi }) => {
      await setKey.mutateAsync(value.key);
      // Write-only: the field is cleared and the key is never shown again.
      formApi.reset();
    },
  });

  return (
    <section className="flex flex-col gap-3">
      <h3 className="text-sm font-medium">
        <Trans>Anthropic API key</Trans>
      </h3>
      <form
        className="flex gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void form.handleSubmit();
        }}
      >
        <form.Field name="key">
          {(field) => (
            <Input
              type="password"
              autoComplete="off"
              spellCheck={false}
              placeholder={
                keyStatus.data?.present ? t`Replace the saved key` : "sk-ant-…"
              }
              value={field.state.value}
              onChange={(event) => field.handleChange(event.target.value)}
              aria-label={t`Anthropic API key`}
            />
          )}
        </form.Field>
        <Button type="submit" disabled={setKey.isPending}>
          <Trans>Save</Trans>
        </Button>
        {keyStatus.data?.present ? (
          <Button
            type="button"
            variant="outline"
            disabled={clearKey.isPending}
            onClick={() => clearKey.mutate()}
          >
            <Trans>Remove</Trans>
          </Button>
        ) : null}
      </form>
      {setKey.error ? (
        <p role="alert" className="text-destructive text-xs">
          {setKey.error.message}
        </p>
      ) : null}
      <p className="text-muted-foreground text-xs">
        <Trans>
          The key is stored in the macOS Keychain and used only by the app's
          Rust core. It is never shown again or sent anywhere except the AI
          provider.
        </Trans>
      </p>
    </section>
  );
}

function UsageLine({
  label,
  counts,
  calls,
  tokens,
}: {
  label: string;
  counts: UsageCounts;
  calls: number;
  tokens: number;
}) {
  const used = counts.input_tokens + counts.output_tokens;
  return (
    <p className="text-sm">
      <span className="font-medium">{label}</span>{" "}
      <Trans>
        {counts.calls} of {calls} calls, {used.toLocaleString()} of{" "}
        {tokens.toLocaleString()} tokens
      </Trans>
    </p>
  );
}

function UsageSection({ status }: { status: EngineStatus }) {
  const { t } = useLingui();
  return (
    <section data-ai-usage-line className="flex flex-col gap-1">
      <h3 className="text-sm font-medium">
        <Trans>Usage</Trans>
      </h3>
      <UsageLine
        label={t`Today:`}
        counts={status.today}
        calls={status.quotas.daily_calls}
        tokens={status.quotas.daily_tokens}
      />
      <UsageLine
        label={t`This month:`}
        counts={status.month}
        calls={status.quotas.monthly_calls}
        tokens={status.quotas.monthly_tokens}
      />
    </section>
  );
}

const QUOTA_FIELDS = [
  "daily_calls",
  "daily_tokens",
  "monthly_calls",
  "monthly_tokens",
  "per_minute",
] as const satisfies ReadonlyArray<keyof Quotas>;

function QuotasSection({ quotas }: { quotas: Quotas }) {
  const { t } = useLingui();
  const setQuotas = useSetEngineQuotas();
  const labels: Record<(typeof QUOTA_FIELDS)[number], string> = {
    daily_calls: t`Calls per day`,
    daily_tokens: t`Tokens per day`,
    monthly_calls: t`Calls per month`,
    monthly_tokens: t`Tokens per month`,
    per_minute: t`Calls per minute`,
  };
  const form = useForm({
    defaultValues: quotas,
    // Rust validates every value against its allowed range.
    onSubmit: ({ value }) => setQuotas.mutateAsync(value),
  });

  return (
    <section className="flex flex-col gap-3">
      <h3 className="text-sm font-medium">
        <Trans>Limits</Trans>
      </h3>
      <form
        className="grid grid-cols-2 gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          void form.handleSubmit();
        }}
      >
        {QUOTA_FIELDS.map((name) => (
          <form.Field key={name} name={name}>
            {(field) => (
              <label className="flex flex-col gap-1 text-sm">
                {labels[name]}
                <Input
                  type="number"
                  min={1}
                  value={field.state.value}
                  onChange={(event) =>
                    field.handleChange(Number(event.target.value))
                  }
                />
              </label>
            )}
          </form.Field>
        ))}
        <div className="col-span-2 flex items-center gap-3">
          <Button
            type="submit"
            variant="outline"
            disabled={setQuotas.isPending}
          >
            <Trans>Save limits</Trans>
          </Button>
          {setQuotas.error ? (
            <span role="alert" className="text-destructive text-xs">
              {setQuotas.error.message}
            </span>
          ) : null}
        </div>
      </form>
    </section>
  );
}
