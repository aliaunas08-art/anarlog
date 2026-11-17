import { t } from "@lingui/core/macro";
import { useCallback, useMemo } from "react";

import { CircleNotch } from "@anlg/ui/components/icons";

import { useSync } from "../context";
import {
  OAuthCalendarSelection,
  useOAuthCalendarSelection,
} from "./calendar-selection";
import { ReconnectRequiredIndicator } from "./status";

import {
  accountKindLabel,
  type LocalAccount,
  localConnectionId,
  type LocalAccountProvider,
  providerLabel,
  useLocalAccountActions,
  useLocalAccounts,
} from "~/accounts/local";
import type { CalendarProvider } from "~/calendar/components/shared";
import {
  allowReconnectedCalendarConnections,
  removeDisconnectedCalendarConnection,
} from "~/services/calendar";

/** Connects calendars through an account signed in on this Mac. */
export function useLocalCalendarConnect(
  config: CalendarProvider,
  provider: LocalAccountProvider,
  onConnectStarted?: () => void,
) {
  const { scheduleSync } = useSync();
  const integrationId = config.nangoIntegrationId ?? "";
  const actions = useLocalAccountActions({
    onConnected: () => {
      allowReconnectedCalendarConnections(integrationId);
      scheduleSync();
    },
    onDisconnected: (account) => {
      void removeDisconnectedCalendarConnection(
        integrationId,
        localConnectionId(account),
      )
        .catch((error: unknown) => {
          console.error(
            "[calendar] failed to remove disconnected calendar data",
            error,
          );
        })
        .finally(scheduleSync);
    },
  });
  const connect = useCallback(() => {
    onConnectStarted?.();
    void actions.connect(provider);
  }, [actions, onConnectStarted, provider]);
  return { ...actions, connect, isConnecting: actions.connecting === provider };
}

export function LocalProviderContent({
  config,
  provider,
  onConnectStarted,
}: {
  config: CalendarProvider;
  provider: LocalAccountProvider;
  onConnectStarted?: () => void;
}) {
  const { data: allAccounts = [], isError } = useLocalAccounts();
  const accounts = useMemo(
    () => allAccounts.filter((account) => account.provider === provider),
    [allAccounts, provider],
  );
  const { connect, cancel, disconnect, isConnecting } = useLocalCalendarConnect(
    config,
    provider,
    onConnectStarted,
  );

  if (isError) {
    return (
      <p className="pt-1 pb-2 text-xs text-red-600">
        {t`Couldn't read your connected accounts from the Keychain`}
      </p>
    );
  }

  const connectButton = (
    <div className="flex items-center gap-2 pt-1 pb-2">
      <button
        type="button"
        onClick={connect}
        disabled={isConnecting}
        className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-xs underline transition-colors disabled:cursor-default disabled:no-underline"
      >
        {isConnecting && (
          <CircleNotch className="size-3 animate-spin" aria-hidden="true" />
        )}
        {isConnecting
          ? t`Waiting for ${providerLabel(provider)} in your browser…`
          : accounts.length > 0
            ? t`Add another ${providerLabel(provider)} account`
            : t`Connect ${config.displayName} Calendar`}
      </button>
      {isConnecting && (
        <button
          type="button"
          onClick={cancel}
          className="text-muted-foreground hover:text-foreground text-xs transition-colors"
        >
          {t`Cancel`}
        </button>
      )}
    </div>
  );

  if (accounts.length === 0) return connectButton;

  return (
    <div className="flex flex-col gap-3 pb-2">
      {accounts
        .filter((account) => account.needs_reconnect)
        .map((account) => (
          <div key={account.id} className="flex flex-col gap-2">
            <div className="flex items-center gap-2 text-xs text-amber-700">
              <ReconnectRequiredIndicator />
              <span>
                {t`Reconnect ${account.email ?? providerLabel(provider)} to keep syncing`}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={connect}
                disabled={isConnecting}
                className="text-muted-foreground hover:text-foreground text-xs underline transition-colors disabled:opacity-50"
              >
                {t`Reconnect`}
              </button>
              <span className="text-muted-foreground text-xs">{t`or`}</span>
              <button
                type="button"
                onClick={() => void disconnect(account)}
                className="text-xs text-red-500 underline transition-colors hover:text-red-700"
              >
                {t`Disconnect`}
              </button>
            </div>
          </div>
        ))}
      <LocalConnectedCalendars
        config={config}
        accounts={accounts}
        onReconnect={connect}
        onDisconnect={disconnect}
      />
      {connectButton}
    </div>
  );
}

function LocalConnectedCalendars({
  config,
  accounts,
  onReconnect,
  onDisconnect,
}: {
  config: CalendarProvider;
  accounts: LocalAccount[];
  onReconnect: () => void;
  onDisconnect: (account: LocalAccount) => Promise<boolean>;
}) {
  const { groups, handleRefresh, handleToggle, isLoading } =
    useOAuthCalendarSelection(config);

  const localGroups = useMemo(
    () =>
      groups.flatMap((group) => {
        const account = accounts.find(
          (item) => localConnectionId(item) === group.id,
        );
        if (!account) return [];
        return [
          {
            ...group,
            account,
            menuItems: [
              {
                id: `reconnect-${account.id}`,
                text: t`Reconnect`,
                action: onReconnect,
              },
              {
                id: `disconnect-${account.id}`,
                text: t`Disconnect`,
                action: () => void onDisconnect(account),
              },
            ],
          },
        ];
      }),
    [accounts, groups, onDisconnect, onReconnect],
  );

  if (localGroups.length === 0) {
    return (
      <p className="text-muted-foreground text-xs">
        {isLoading ? t`Loading calendars…` : t`No calendars found yet`}
      </p>
    );
  }

  // One block per account, headed by which account it is: one email can
  // have a personal and a work Microsoft account.
  const byAccount = accounts.flatMap((account) => {
    const accountGroups = localGroups.filter(
      (group) => group.account.id === account.id,
    );
    return accountGroups.length > 0 ? [{ account, groups: accountGroups }] : [];
  });

  return (
    <div className="flex flex-col gap-3">
      {byAccount.map(({ account, groups: accountGroups }) => {
        const kind = accountKindLabel(account);
        const who = account.email ?? account.name ?? account.id;
        // Google lists the email as the calendar owner already.
        const showHeader =
          kind !== null ||
          !accountGroups.some((group) => group.sourceName === who);
        return (
          <div key={account.id} className="flex flex-col gap-1">
            {showHeader ? (
              <p
                className="text-muted-foreground truncate text-xs"
                title={account.id}
              >
                {kind ? `${who} · ${kind}` : who}
              </p>
            ) : null}
            <OAuthCalendarSelection
              groups={accountGroups}
              onToggle={handleToggle}
              onRefresh={handleRefresh}
              isLoading={isLoading}
            />
          </div>
        );
      })}
    </div>
  );
}
