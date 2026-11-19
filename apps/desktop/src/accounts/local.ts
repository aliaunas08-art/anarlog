import { t } from "@lingui/core/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useRef, useSyncExternalStore } from "react";

import {
  type Account,
  commands as accountsCommands,
  type Provider,
} from "@anlg/plugin-accounts";
import { toast } from "@anlg/ui/components/ui/toast";

export type { Account as LocalAccount, Provider as LocalAccountProvider };

export const LOCAL_CONNECTION_PREFIX = "local:";

const ACCOUNTS_KEY = ["local-accounts"] as const;
const PROVIDERS_KEY = ["local-account-providers"] as const;

/** Calendar provider id → the account it signs in with. */
export function localProviderForCalendar(
  calendarProviderId: string,
): Provider | null {
  if (calendarProviderId === "google") return "google";
  if (calendarProviderId === "outlook") return "microsoft";
  return null;
}

export function localConnectionId(account: Account): string {
  return `${LOCAL_CONNECTION_PREFIX}${account.id}`;
}

/** Providers whose sign-in is set up in this build (has a client ID). */
export function useLocalAccountProviders() {
  return useQuery({
    queryKey: PROVIDERS_KEY,
    queryFn: () => accountsCommands.configuredProviders(),
    staleTime: Infinity,
  });
}

export function useLocalAccounts() {
  return useQuery({
    queryKey: ACCOUNTS_KEY,
    queryFn: async () => {
      const result = await accountsCommands.listAccounts();
      if (result.status === "error") throw new Error(result.error.message);
      return result.data;
    },
  });
}

/** "Personal account" or "Work or school account" for a Microsoft account. */
export function accountKindLabel(account: Account): string | null {
  switch (account.kind) {
    case "personal":
      return t`Personal account`;
    case "work_or_school":
      return t`Work or school account`;
    default:
      return null;
  }
}

function accountLabel(account: Account): string {
  const who = account.email ?? account.name ?? account.id;
  const kind = accountKindLabel(account);
  return kind ? `${who} (${kind})` : who;
}

/**
 * A reconnect landed on a different account than the last one (one email can
 * have a personal and a work account, or the browser picked another saved
 * account). Say so instead of quietly syncing a different calendar.
 */
function warnAccountChanged(
  account: Account,
  replaced: Account,
  disconnect: () => void,
) {
  const name = providerLabel(account.provider);
  toast.warning(t`Connected a different ${name} account than last time`, {
    description: t`Now: ${accountLabel(account)}, id ${account.id}. Before: ${accountLabel(replaced)}, id ${replaced.id}. If this isn't the account you meant, disconnect it and connect again, choosing the other account.`,
    duration: Infinity,
    closeButton: true,
    action: { label: t`Keep`, onClick: () => {} },
    secondaryAction: { label: t`Disconnect`, onClick: disconnect },
  });
}

export function providerLabel(provider: Provider): string {
  return provider === "google" ? "Google" : "Microsoft";
}

function connectErrorMessage(code: string, provider: Provider): string | null {
  const name = providerLabel(provider);
  switch (code) {
    case "cancelled":
      return null;
    case "denied":
      return t`${name} access wasn't granted.`;
    case "missing_scope":
      return t`Notes needs access to your ${name} calendar. Connect again and leave calendar access ticked.`;
    case "timed_out":
      return t`${name} sign-in timed out. Try again when you're ready.`;
    case "not_configured":
      return t`${name} sign-in isn't set up in this build.`;
    case "network":
      return t`Couldn't reach ${name}. Check your connection and try again.`;
    case "keychain":
      return t`Couldn't save the ${name} account to the Keychain.`;
    default:
      return t`Couldn't connect your ${name} account.`;
  }
}

// The provider whose browser sign-in is in progress, shared by every view.
let connectingProvider: Provider | null = null;
const connectingListeners = new Set<() => void>();
function setConnectingProvider(next: Provider | null) {
  connectingProvider = next;
  connectingListeners.forEach((listener) => listener());
}
function subscribeConnecting(listener: () => void) {
  connectingListeners.add(listener);
  return () => {
    connectingListeners.delete(listener);
  };
}
const getConnecting = () => connectingProvider;

/**
 * Connect and disconnect accounts. `connecting` is the provider whose browser
 * sign-in is in progress.
 */
export function useLocalAccountActions(options?: {
  onConnected?: (account: Account) => void;
  onDisconnected?: (account: Account) => void;
}) {
  const queryClient = useQueryClient();
  const connecting = useSyncExternalStore(
    subscribeConnecting,
    getConnecting,
    getConnecting,
  );
  const onConnected = options?.onConnected;
  const onDisconnected = options?.onDisconnected;
  const disconnectRef = useRef<((account: Account) => Promise<boolean>) | null>(
    null,
  );

  const connect = useCallback(
    async (provider: Provider) => {
      setConnectingProvider(provider);
      try {
        const result = await accountsCommands.connectAccount(provider);
        if (result.status === "error") {
          const message = connectErrorMessage(result.error.code, provider);
          if (message) toast.error(message);
          return null;
        }
        await queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY });
        const { account, replaced } = result.data;
        if (replaced) {
          warnAccountChanged(account, replaced, () => {
            void disconnectRef.current?.(account);
          });
        } else {
          const who = account.email ?? account.name;
          toast.success(
            who
              ? t`Connected ${who}`
              : t`Connected your ${providerLabel(provider)} account`,
          );
        }
        onConnected?.(account);
        return account;
      } finally {
        if (connectingProvider === provider) setConnectingProvider(null);
      }
    },
    [onConnected, queryClient],
  );

  const cancel = useCallback(() => {
    void accountsCommands.cancelConnect();
  }, []);

  const disconnect = useCallback(
    async (account: Account) => {
      const result = await accountsCommands.disconnectAccount(account.id);
      if (result.status === "error") {
        toast.error(t`Couldn't disconnect ${account.email ?? "the account"}.`);
        return false;
      }
      await queryClient.invalidateQueries({ queryKey: ACCOUNTS_KEY });
      onDisconnected?.(account);
      return true;
    },
    [onDisconnected, queryClient],
  );
  disconnectRef.current = disconnect;

  return { connect, cancel, disconnect, connecting };
}
