import { useLingui } from "@lingui/react/macro";

import { useFileVaultState } from "./status";

import { SettingRow } from "~/settings/setting-row";

// ASPC C2.1: shows whether FileVault protects the library on disk.
export function DiskEncryptionRow() {
  const { t } = useLingui();
  const { data: state } = useFileVaultState();
  if (!state) return null;

  const label = state === "on" ? t`On` : state === "off" ? t`Off` : t`Unknown`;
  const description =
    state === "on"
      ? t`FileVault encrypts this Mac's disk, including your library.`
      : state === "off"
        ? t`FileVault is off: your library is stored without disk encryption. Turn it on in System Settings → Privacy & Security.`
        : t`Couldn't check FileVault. Check it in System Settings → Privacy & Security.`;

  return (
    <SettingRow
      title={t`Disk encryption (FileVault)`}
      description={description}
      controlWidth="content"
    >
      {(labelProps) => (
        <span
          {...labelProps}
          className={
            state === "on"
              ? "text-muted-foreground text-sm"
              : "text-sm font-medium text-amber-600"
          }
        >
          {label}
        </span>
      )}
    </SettingRow>
  );
}
