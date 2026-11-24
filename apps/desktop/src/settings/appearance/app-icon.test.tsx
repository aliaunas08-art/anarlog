import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  platform: vi.fn(() => "macos"),
  setAppIcon: vi.fn(),
  applyAppIconPreference: vi.fn(),
  appIcon: "default",
  theme: "system",
  appIdentifier: "com.hyprnote.stable" as string | undefined,
  billing: {
    isPro: true,
    isUpgradingToPro: false,
    upgradeToPro: vi.fn(),
  },
  toastWarning: vi.fn(),
}));

vi.mock("@tanstack/react-query", () => ({
  useQuery: () => ({ data: mocks.appIdentifier }),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getIdentifier: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: mocks.platform,
}));

vi.mock("~/settings/queries", () => ({
  useSetSettingValue: () => mocks.setAppIcon,
}));

vi.mock("~/auth/billing-context", () => ({
  useBillingAccess: () => mocks.billing,
}));

vi.mock("@anlg/ui/components/ui/toast", () => ({
  toast: { warning: mocks.toastWarning },
}));

vi.mock("~/shared/config", () => ({
  useConfigValue: (key: string) =>
    key === "theme" ? mocks.theme : mocks.appIcon,
}));

vi.mock("~/shared/theme/provider", () => ({
  applyAppIconPreference: mocks.applyAppIconPreference,
}));

import { AppIconSelector } from "./app-icon";

describe("AppIconSelector", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    mocks.platform.mockReturnValue("macos");
    mocks.appIcon = "default";
    mocks.theme = "system";
    mocks.appIdentifier = "com.hyprnote.stable";
    mocks.billing.isPro = true;
    mocks.billing.isUpgradingToPro = false;
    mocks.toastWarning.mockClear();
  });

  it.each([["system"], ["dark"]])(
    "shows no picker with only the Timbre icon (%s theme)",
    (theme) => {
      mocks.theme = theme;
      const { container } = render(<AppIconSelector />);

      expect(container.textContent).toBe("");
      expect(screen.queryByRole("radio")).toBeNull();
      expect(mocks.applyAppIconPreference).not.toHaveBeenCalled();
      expect(mocks.setAppIcon).not.toHaveBeenCalled();
    },
  );

  it("offers none of the old Anarlog icons, even if one was saved", () => {
    mocks.appIcon = "walnut";

    render(<AppIconSelector />);

    for (const name of ["Blueprint", "Anagram", "Walnut", "Production"]) {
      expect(screen.queryByRole("radio", { name })).toBeNull();
    }
  });
});
