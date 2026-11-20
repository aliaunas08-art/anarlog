import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LocalProviderContent } from "./local-provider-content";

import type { CalendarProvider } from "~/calendar/components/shared";

const accountsMock = vi.hoisted(() => ({
  configuredProviders: vi.fn(),
  listAccounts: vi.fn(),
  connectAccount: vi.fn(),
  cancelConnect: vi.fn(),
  disconnectAccount: vi.fn(),
}));
const syncMock = vi.hoisted(() => ({ scheduleSync: vi.fn() }));
const calendarMock = vi.hoisted(() => ({
  allowReconnectedCalendarConnections: vi.fn(),
  removeDisconnectedCalendarConnection: vi.fn(() => Promise.resolve()),
}));
const selectionMock = vi.hoisted(() => ({
  groups: [] as Array<{
    id?: string;
    sourceName: string;
    calendars: Array<{
      id: string;
      title: string;
      color: string;
      enabled: boolean;
    }>;
  }>,
}));

vi.mock("@anlg/plugin-accounts", () => ({ commands: accountsMock }));
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));
vi.mock("@anlg/ui/components/ui/toast", () => ({
  toast: toastMock,
}));
vi.mock("./status", () => ({ ReconnectRequiredIndicator: () => null }));
vi.mock("../context", () => ({ useSync: () => syncMock }));
vi.mock("~/services/calendar", () => calendarMock);
vi.mock("~/shared/hooks/useNativeContextMenu", () => ({
  useNativeContextMenu: () => vi.fn(),
}));
vi.mock("./calendar-selection", () => ({
  useOAuthCalendarSelection: () => ({
    groups: selectionMock.groups,
    handleRefresh: vi.fn(),
    handleToggle: vi.fn(),
    isLoading: false,
  }),
  OAuthCalendarSelection: ({
    groups,
  }: {
    groups: Array<{
      sourceName: string;
      menuItems?: Array<{ id: string; text: string; action: () => void }>;
    }>;
  }) => (
    <div>
      {groups.map((group) => (
        <div key={group.sourceName}>
          <span>{group.sourceName}</span>
          {group.menuItems?.map((item) => (
            <button key={item.id} type="button" onClick={item.action}>
              {`${item.text} ${group.sourceName}`}
            </button>
          ))}
        </div>
      ))}
    </div>
  ),
}));

const google = {
  id: "google",
  displayName: "Google",
  nangoIntegrationId: "google-calendar",
} as CalendarProvider;

const account = {
  id: "google-1234",
  provider: "google" as const,
  email: "sam@example.com",
  name: "Sam Lee",
  picture_url: null,
  connected_at: "2026-11-16T12:00:00Z",
  needs_reconnect: false,
};

function wrap(children: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("LocalProviderContent", () => {
  beforeEach(() => {
    selectionMock.groups = [];
    accountsMock.listAccounts.mockResolvedValue({ status: "ok", data: [] });
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("connects through the browser, then syncs calendars", async () => {
    accountsMock.connectAccount.mockResolvedValue({
      status: "ok",
      data: { account, replaced: null },
    });
    render(wrap(<LocalProviderContent config={google} provider="google" />));

    fireEvent.click(await screen.findByText("Connect Google Calendar"));
    await waitFor(() =>
      expect(accountsMock.connectAccount).toHaveBeenCalledWith("google"),
    );
    await waitFor(() => expect(syncMock.scheduleSync).toHaveBeenCalled());
    expect(
      calendarMock.allowReconnectedCalendarConnections,
    ).toHaveBeenCalledWith("google-calendar");
  });

  it("lists the account's calendars and disconnects it", async () => {
    accountsMock.listAccounts.mockResolvedValue({
      status: "ok",
      data: [account],
    });
    accountsMock.disconnectAccount.mockResolvedValue({
      status: "ok",
      data: null,
    });
    selectionMock.groups = [
      {
        id: "local:google-1234",
        sourceName: "sam@example.com",
        calendars: [],
      },
      // A server connection's calendars are not shown here.
      { id: "conn_server", sourceName: "other@example.com", calendars: [] },
    ];
    render(wrap(<LocalProviderContent config={google} provider="google" />));

    expect(await screen.findByText("sam@example.com")).toBeTruthy();
    expect(screen.queryByText("other@example.com")).toBeNull();
    expect(screen.getByText("Add another Google account")).toBeTruthy();

    fireEvent.click(screen.getByText("Disconnect sam@example.com"));
    await waitFor(() =>
      expect(accountsMock.disconnectAccount).toHaveBeenCalledWith(
        "google-1234",
      ),
    );
    await waitFor(() =>
      expect(
        calendarMock.removeDisconnectedCalendarConnection,
      ).toHaveBeenCalledWith("google-calendar", "local:google-1234"),
    );
  });

  it("asks to reconnect an account Google stopped accepting", async () => {
    accountsMock.listAccounts.mockResolvedValue({
      status: "ok",
      data: [{ ...account, needs_reconnect: true }],
    });
    render(wrap(<LocalProviderContent config={google} provider="google" />));
    expect(
      await screen.findByText("Reconnect sam@example.com to keep syncing"),
    ).toBeTruthy();
  });

  it("names the Microsoft account and its type above its calendars", async () => {
    const outlook = {
      id: "outlook",
      displayName: "Outlook",
      nangoIntegrationId: "outlook",
    } as CalendarProvider;
    accountsMock.listAccounts.mockResolvedValue({
      status: "ok",
      data: [
        {
          ...account,
          id: "microsoft-8557",
          provider: "microsoft" as const,
          email: "ali@outlook.com",
          name: "Ali",
          kind: "personal" as const,
        },
      ],
    });
    selectionMock.groups = [
      { id: "local:microsoft-8557", sourceName: "Ali", calendars: [] },
      { id: "local:microsoft-8557", sourceName: "Your family", calendars: [] },
    ];
    render(
      wrap(<LocalProviderContent config={outlook} provider="microsoft" />),
    );
    expect(
      await screen.findByText("ali@outlook.com · Personal account"),
    ).toBeTruthy();
    expect(screen.getByText("Your family")).toBeTruthy();
  });

  it("warns when a reconnect lands on a different account", async () => {
    const replaced = { ...account, id: "google-0001" };
    accountsMock.connectAccount.mockResolvedValue({
      status: "ok",
      data: { account, replaced },
    });
    render(wrap(<LocalProviderContent config={google} provider="google" />));
    fireEvent.click(await screen.findByText("Connect Google Calendar"));
    await waitFor(() =>
      expect(toastMock.warning).toHaveBeenCalledWith(
        "Connected a different Google account than last time",
        expect.objectContaining({
          description: expect.stringContaining("google-0001"),
        }),
      ),
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });
});
