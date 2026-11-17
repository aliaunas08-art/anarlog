import { useLingui } from "@lingui/react/macro";
import { platform } from "@tauri-apps/plugin-os";

import { useMountEffect } from "@anlg/ui/hooks/use-mount-effect";

import { RetentionSetting } from "./retention";

import { startRustLockSync, useAppLock } from "~/lock/store";
import { privacyMessages } from "~/settings/general/app-settings";
import { SettingsPageTitle } from "~/settings/page-title";
import {
  useSetSettingValues,
  useStoredSettingValuesQuery,
} from "~/settings/queries";
import { SettingSwitchRow } from "~/settings/setting-row";
import { APP_NAME } from "~/shared/app-name";
import { resolveConfigValue } from "~/shared/config";

export function SettingsPrivacy() {
  const { i18n, t } = useLingui();
  const settingsQuery = useStoredSettingValuesQuery();
  const setSettingValues = useSetSettingValues();
  const available = useAppLock((state) => state.available);
  const authenticating = useAppLock((state) => state.authenticating);
  const lockAppEnabled = useAppLock((state) => state.lockEnabled === true);
  const setLockEnabled = useAppLock((state) => state.setLockEnabled);
  const refreshAvailability = useAppLock((state) => state.refreshAvailability);

  useMountEffect(() => {
    void refreshAvailability();
    void startRustLockSync();
  });

  if (settingsQuery.error) {
    throw settingsQuery.error;
  }
  if (settingsQuery.isLoading || !settingsQuery.data) {
    return null;
  }

  const posthogEnabled = resolveConfigValue(
    "telemetry_consent",
    settingsQuery.data,
  );
  const sentryEnabled = resolveConfigValue(
    "crash_reporting_consent",
    settingsQuery.data,
  );
  const authAvailable = available === true;
  const lockAppDescription = !authAvailable
    ? t`Device authentication is not available on this computer.`
    : platform() === "windows"
      ? t`Require Windows Hello face, PIN, or password when opening ${APP_NAME}.`
      : t`Require Touch ID or your password when opening ${APP_NAME}.`;

  return (
    <div className="flex flex-col gap-8">
      <SettingsPageTitle title={i18n._(privacyMessages.title)} />

      <section className="flex flex-col gap-4">
        <SettingSwitchRow
          title={t`Lock app`}
          description={lockAppDescription}
          checked={lockAppEnabled && authAvailable}
          disabled={!authAvailable || authenticating}
          onChange={(next) => {
            void (async () => {
              const canAuth = await refreshAvailability();
              if (!canAuth) return;
              // Rust runs the device authentication and stores the setting
              // (ASPC A4.2); the webview never writes `lock_app` itself.
              await setLockEnabled(next);
            })();
          }}
        />
        <SettingSwitchRow
          title={i18n._(privacyMessages.posthogTitle)}
          description={i18n._(privacyMessages.posthogDescription)}
          checked={posthogEnabled}
          onChange={(telemetryConsent) => {
            setSettingValues({ telemetry_consent: telemetryConsent });
          }}
        />
        <SettingSwitchRow
          title={t`Error`}
          description={t`Send sanitized crash and error reports to help improve ${APP_NAME}.`}
          checked={sentryEnabled}
          onChange={(crashReportingConsent) => {
            setSettingValues({
              crash_reporting_consent: crashReportingConsent,
            });
          }}
        />
      </section>

      <section className="flex flex-col gap-4">
        <RetentionSetting />
      </section>
    </div>
  );
}
