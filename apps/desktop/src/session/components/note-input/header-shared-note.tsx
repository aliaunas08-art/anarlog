import { useLingui } from "@lingui/react/macro";

import { Users } from "@anlg/ui/components/icons";

import { IconHeaderView, useHeaderViewVariant } from "./header-shared";

export function HeaderViewShared({
  isActive,
  onClick,
}: {
  isActive: boolean;
  onClick: () => void;
}) {
  const { t } = useLingui();
  const glass = useHeaderViewVariant() === "glass";

  return (
    <IconHeaderView
      isActive={isActive}
      label={t`Shared summary`}
      icon={glass ? null : <Users className="size-3.5" />}
      onClick={onClick}
      size="tray"
    />
  );
}
