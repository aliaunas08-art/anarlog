import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";

import { Input } from "@anlg/ui/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@anlg/ui/components/ui/select";

import { SETTING_CONTROL_CLASS, SettingRow } from "~/settings/setting-row";

const DEFAULT_DAYS = 90;
const MAX_DAYS = 36_500;

export function normalizeRetentionDays(value: unknown): number {
  const days = Math.floor(Number(value));
  return Number.isFinite(days) && days >= 1 && days <= MAX_DAYS ? days : 0;
}

// ASPC D3: "Keep meetings forever" (0, the default) or "Delete meetings
// older than N days". Rust applies it at startup and every 6 hours through
// the permanent delete, skipping a meeting being recorded.
export function RetentionSetting({
  value,
  onChange: setDays,
}: {
  value: unknown;
  onChange: (days: number) => void;
}) {
  const { t } = useLingui();
  const stored = normalizeRetentionDays(value);
  const [draft, setDraft] = useState<string | null>(null);
  const mode = stored > 0 ? "delete" : "forever";

  const commitDraft = () => {
    if (draft === null) return;
    const days = normalizeRetentionDays(draft);
    setDraft(null);
    if (days > 0 && days !== stored) setDays(days);
  };

  return (
    <SettingRow
      title={<Trans>Meeting retention</Trans>}
      description={
        <Trans>
          Older meetings are deleted permanently, with their notes, transcripts,
          audio and attachments, at startup and every 6 hours. A meeting being
          recorded is never deleted.
        </Trans>
      }
    >
      {(labelProps) => (
        <div className="flex items-center gap-2">
          <Select
            value={mode}
            onValueChange={(next) =>
              setDays(next === "forever" ? 0 : DEFAULT_DAYS)
            }
          >
            <SelectTrigger {...labelProps} className={SETTING_CONTROL_CLASS}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="forever">{t`Keep meetings forever`}</SelectItem>
              <SelectItem value="delete">{t`Delete meetings older than…`}</SelectItem>
            </SelectContent>
          </Select>
          {mode === "delete" ? (
            <label className="flex items-center gap-1 text-sm">
              <Input
                type="number"
                min={1}
                max={MAX_DAYS}
                aria-label={t`Days to keep meetings`}
                className="w-20"
                value={draft ?? String(stored)}
                onChange={(event) => setDraft(event.target.value)}
                onBlur={commitDraft}
                onKeyDown={(event) => {
                  if (event.key === "Enter") commitDraft();
                }}
              />
              <Trans>days</Trans>
            </label>
          ) : null}
        </div>
      )}
    </SettingRow>
  );
}
