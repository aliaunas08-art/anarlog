import { Trans, useLingui } from "@lingui/react/macro";

import { Button } from "@anlg/ui/components/ui/button";

import { useTabs } from "~/store/zustand/tabs";

// ASPC D4.1: the privacy policy and terms are not written yet. These are
// deliberate placeholders with no URL; nothing here is legal text.
// TODO(ASPC D4.1): replace with the real documents' URLs once they exist,
// and open them with the opener plugin.
export const PRIVACY_POLICY_URL: string | null = null;
export const TERMS_URL: string | null = null;

export function LegalLinks() {
  const { t } = useLingui();
  const openNew = useTabs((state) => state.openNew);

  return (
    <section className="flex flex-col gap-3">
      <div>
        <h3 className="mb-1 text-sm font-medium">
          <Trans>Your data</Trans>
        </h3>
        <p className="text-muted-foreground text-xs">
          <Trans>
            See where your meetings are stored and what leaves this Mac.
          </Trans>
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            openNew({ type: "settings", state: { tab: "data-locations" } })
          }
        >
          <Trans>Where your data lives</Trans>
        </Button>
        <PlaceholderLink label={t`Privacy Policy`} />
        <PlaceholderLink label={t`Terms of Use`} />
      </div>
    </section>
  );
}

function PlaceholderLink({ label }: { label: string }) {
  const { t } = useLingui();
  return (
    <Button
      variant="outline"
      size="sm"
      disabled
      title={t`Placeholder: not written yet`}
    >
      {t`${label} (placeholder, not written yet)`}
    </Button>
  );
}
