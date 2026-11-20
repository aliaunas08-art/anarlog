import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";

import { commands } from "@anlg/plugin-local-auth";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@anlg/ui/components/ui/select";
import { toast } from "@anlg/ui/components/ui/toast";
import { useMountEffect } from "@anlg/ui/hooks/use-mount-effect";

import { SETTING_CONTROL_CLASS, SettingRow } from "~/settings/setting-row";

// ASPC A4.4: relock after this many minutes with no keyboard or mouse input.
// Rust watches the idle time and stores the value; changing it asks for
// Touch ID or the password, so the webview can't quietly turn it off.
export function IdleRelockSetting({ disabled }: { disabled: boolean }) {
  const { t } = useLingui();
  const [minutes, setMinutes] = useState<number | null>(null);
  const [choices, setChoices] = useState<number[]>([]);
  const [saving, setSaving] = useState(false);

  useMountEffect(() => {
    void commands.appLockSettings().then((result) => {
      if (result.status === "ok") {
        setMinutes(result.data.idleMinutes);
        setChoices(result.data.idleMinuteChoices);
      }
    });
  });

  if (minutes === null) return null;

  const label = (value: number) =>
    value === 0
      ? t`Never`
      : value === 1
        ? t`After 1 minute`
        : value === 60
          ? t`After 1 hour`
          : t`After ${value} minutes`;

  return (
    <SettingRow
      title={<Trans>Lock when idle</Trans>}
      description={
        <Trans>
          Lock the app after this long with no keyboard or mouse activity. A
          recording in progress is never interrupted.
        </Trans>
      }
    >
      {(labelProps) => (
        <Select
          value={String(minutes)}
          disabled={disabled || saving}
          onValueChange={(next) => {
            const requested = Number(next);
            setSaving(true);
            void commands
              .setAppLockIdleMinutes(requested)
              .then((result) => {
                if (result.status === "error") {
                  toast.error(t`Idle lock wasn't changed`, {
                    description: result.error,
                  });
                  return;
                }
                setMinutes(result.data);
                if (result.data !== requested) {
                  toast.error(t`Idle lock wasn't changed`, {
                    description: t`Touch ID or your password wasn't confirmed. Try again and confirm the macOS prompt.`,
                  });
                }
              })
              .finally(() => setSaving(false));
          }}
        >
          <SelectTrigger {...labelProps} className={SETTING_CONTROL_CLASS}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {choices.map((value) => (
              <SelectItem key={value} value={String(value)}>
                {label(value)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </SettingRow>
  );
}
