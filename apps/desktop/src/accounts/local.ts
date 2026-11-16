import { t } from "@lingui/core/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useSyncExternalStore } from "react";

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

export function isLocalConnectionId(connectionId: string): boolean {
  return connectionId.startsWith(LOCAL_CONNECTION_PREFIX);
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
        const who = result.data.email ?? result.data.name;
        toast.success(
          who
            ? t`Connected ${who}`
            : t`Connected your ${providerLabel(provider)} account`,
        );
        onConnected?.(result.data);
        return result.data;
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

  return { connect, cancel, disconnect, connecting };
}
