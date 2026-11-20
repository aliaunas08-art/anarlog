import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  pickSavePath: vi.fn(),
  pickOpenPaths: vi.fn(),
  backupLibrary: vi.fn(),
  restoreLibrary: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("@anlg/plugin-fs2", () => ({
  commands: {
    pickSavePath: mocks.pickSavePath,
    pickOpenPaths: mocks.pickOpenPaths,
  },
}));
vi.mock("@anlg/plugin-session", () => ({
  commands: {
    backupLibrary: mocks.backupLibrary,
    restoreLibrary: mocks.restoreLibrary,
  },
}));
vi.mock("@anlg/ui/components/ui/toast", () => ({
  toast: Object.assign(vi.fn(), {
    success: mocks.success,
    error: mocks.error,
  }),
}));
vi.mock("~/shared/relaunch", () => ({
  scheduleAutomaticRelaunch: mocks.relaunch,
}));
vi.mock("~/store/zustand/listener/instance", () => ({
  listenerStore: { getState: () => ({ live: { status: "inactive" } }) },
}));

import { BackupSettings } from "./backup";

function renderSettings() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <BackupSettings />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(cleanup);

test("backs up to the path the user picked in the native dialog", async () => {
  mocks.pickSavePath.mockResolvedValue({
    status: "ok",
    data: "/Users/me/Backups/library.anarlogbackup",
  });
  mocks.backupLibrary.mockResolvedValue({
    status: "ok",
    data: { meetings: 3, files: 5, bytes: 100 },
  });
  renderSettings();

  fireEvent.click(screen.getByRole("button", { name: "Back up library…" }));

  await vi.waitFor(() => expect(mocks.success).toHaveBeenCalledOnce());
  expect(mocks.backupLibrary).toHaveBeenCalledWith(
    "/Users/me/Backups/library.anarlogbackup",
  );
});

test("a cancelled confirmation is silent and nothing is restored", async () => {
  mocks.pickOpenPaths.mockResolvedValue({
    status: "ok",
    data: ["/Users/me/library.anarlogbackup"],
  });
  mocks.restoreLibrary.mockResolvedValue({
    status: "error",
    error: "backup.cancelled",
  });
  renderSettings();

  fireEvent.click(screen.getByRole("button", { name: "Restore from backup…" }));

  await vi.waitFor(() => expect(mocks.restoreLibrary).toHaveBeenCalled());
  await Promise.resolve();
  expect(mocks.error).not.toHaveBeenCalled();
  expect(mocks.success).not.toHaveBeenCalled();
  expect(mocks.relaunch).not.toHaveBeenCalled();
});
