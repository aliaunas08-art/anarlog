import { useLingui } from "@lingui/react/macro";

import { toast } from "@anlg/ui/components/ui/toast";
import { useMountEffect } from "@anlg/ui/hooks/use-mount-effect";

import { getFileVaultState } from "./status";

import { useTabs } from "~/store/zustand/tabs";

const TOAST_ID = "aspc:filevault-off";
let shownThisLaunch = false;

// ASPC C2.1: a non-blocking warning when FileVault is off. Shown once per
// launch; it never blocks recording or the library.
export function FileVaultWarning() {
  const { t } = useLingui();
  const openNew = useTabs((state) => state.openNew);

  useMountEffect(() => {
    let cancelled = false;
    void getFileVaultState().then((state) => {
      if (cancelled || state !== "off" || shownThisLaunch) return;
      shownThisLaunch = true;
      toast.warning(t`FileVault is off`, {
        id: TOAST_ID,
        duration: Infinity,
        description: t`Your meetings are stored on this Mac without disk encryption. Turn on FileVault in System Settings → Privacy & Security.`,
        action: {
          label: t`Details`,
          onClick: () => {
            openNew({ type: "settings", state: { tab: "privacy" } });
            toast.dismiss(TOAST_ID);
          },
        },
      });
    });
    return () => {
      cancelled = true;
    };
  });

  return null;
}
