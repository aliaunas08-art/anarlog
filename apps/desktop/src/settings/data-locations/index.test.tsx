import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  values: {} as Record<string, unknown>,
  getDataLocations: vi.fn(),
}));

vi.mock("~/settings/queries", () => ({
  useStoredSettingValuesQuery: () => ({
    data: {
      values: mocks.values,
      hasValues: new Set(Object.keys(mocks.values)),
    },
    isLoading: false,
    error: null,
  }),
}));
vi.mock("~/types/tauri.gen", () => ({
  commands: { getDataLocations: mocks.getDataLocations },
}));
vi.mock("~/data-protection/disk-encryption-row", () => ({
  DiskEncryptionRow: () => null,
}));

import { SettingsDataLocations, sttStaysOnDevice } from ".";

function renderScreen() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsDataLocations />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  mocks.values = {};
  mocks.getDataLocations.mockResolvedValue({
    status: "ok",
    data: {
      data_dir: "/Users/me/Library/Application Support/app",
      database: "/Users/me/Library/Application Support/app/app.db",
      audit_log: "/Users/me/Library/Application Support/app/audit.db",
      vault: "/Users/me/Library/Application Support/app",
      sessions: "/Users/me/Library/Application Support/app/sessions",
      search_index: "/Users/me/Library/Application Support/app/search_index",
      logs: "/Users/me/Library/Logs/app",
    },
  });
});
afterEach(cleanup);

test("lists the real local paths Rust reports", async () => {
  renderScreen();
  expect(
    await screen.findByText("/Users/me/Library/Application Support/app/app.db"),
  ).toBeTruthy();
  expect(
    screen.getByText("/Users/me/Library/Application Support/app/sessions"),
  ).toBeTruthy();
});

test("follows the settings: analytics off by default, cloud transcription named", () => {
  mocks.values = { current_stt_provider: "deepgram" };
  renderScreen();
  expect(screen.getAllByText("Off. Nothing is sent.")).toHaveLength(2);
  expect(screen.getByText(/meeting audio is sent to Deepgram/)).toBeTruthy();
  expect(
    screen.getByText(/never used for analytics, telemetry, product research/),
  ).toBeTruthy();
});

test("on-device transcription keeps audio on the Mac", () => {
  expect(sttStaysOnDevice("apple_speech")).toBe(true);
  expect(sttStaysOnDevice("deepgram")).toBe(false);
  mocks.values = {
    current_stt_provider: "apple_speech",
    analytics_opt_in: true,
  };
  renderScreen();
  expect(screen.getByText(/audio stays here/)).toBeTruthy();
  expect(screen.getByText(/event names, app version and counts/)).toBeTruthy();
});
