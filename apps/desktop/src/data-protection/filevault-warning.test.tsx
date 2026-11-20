import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("~/types/tauri.gen", () => ({
  commands: { getDataProtectionStatus: mocks.status },
}));

vi.mock("@anlg/ui/components/ui/toast", () => ({
  toast: Object.assign(vi.fn(), { warning: mocks.warning, dismiss: vi.fn() }),
}));

vi.mock("~/store/zustand/tabs", () => ({
  useTabs: (selector: (state: { openNew: () => void }) => unknown) =>
    selector({ openNew: vi.fn() }),
}));

beforeEach(() => {
  vi.resetModules();
  mocks.status.mockReset();
  mocks.warning.mockReset();
});

afterEach(cleanup);

test("warns once when FileVault is off and never blocks", async () => {
  mocks.status.mockResolvedValue({ filevault: "off" });
  const { FileVaultWarning } = await import("./filevault-warning");

  const view = render(<FileVaultWarning />);
  await vi.waitFor(() => expect(mocks.warning).toHaveBeenCalledOnce());
  expect(view.container.innerHTML).toBe("");

  render(<FileVaultWarning />);
  await Promise.resolve();
  expect(mocks.warning).toHaveBeenCalledOnce();
});

test("stays quiet when FileVault is on or the check fails", async () => {
  mocks.status.mockResolvedValueOnce({ filevault: "on" });
  mocks.status.mockRejectedValueOnce(new Error("no fdesetup"));
  const { FileVaultWarning } = await import("./filevault-warning");

  render(<FileVaultWarning />);
  render(<FileVaultWarning />);
  await vi.waitFor(() => expect(mocks.status).toHaveBeenCalledTimes(2));
  await Promise.resolve();
  expect(mocks.warning).not.toHaveBeenCalled();
});
