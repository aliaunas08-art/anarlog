import { fetch as tauriFetch } from "@tauri-apps/plugin-http";

import { commands as miscCommands } from "@anlg/plugin-misc";

const SCOPE_ERROR = "url not allowed on the configured scope";

export function isHttpScopeError(error: unknown) {
  return String(error).includes(SCOPE_ERROR);
}

// The http plugin only reaches the hosts in the capability scope (ASPC A3.3).
// A provider base URL the user typed themselves is allowed after a native
// confirmation in Rust, then the request is retried once.
export const scopedFetch: typeof fetch = async (input, init) => {
  try {
    return await tauriFetch(input, init);
  } catch (error) {
    if (!isHttpScopeError(error)) throw error;
    const url = input instanceof Request ? input.url : input.toString();
    const allowed = await miscCommands.allowHttpOrigin(url);
    if (allowed.status !== "ok" || !allowed.data) throw error;
    return tauriFetch(input, init);
  }
};

export const providerFetch: typeof fetch = (input, init) => {
  const url = new URL(input instanceof Request ? input.url : input);
  if (
    (url.protocol === "http:" || url.protocol === "https:") &&
    ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
  ) {
    const headers = new Headers(
      init?.headers ?? (input instanceof Request ? input.headers : undefined),
    );
    if (!headers.has("Origin")) {
      // Tauri's unsafe-headers transport removes an empty Origin instead of
      // injecting the webview origin, which local servers can reject.
      headers.set("Origin", "");
      return scopedFetch(input, { ...init, headers });
    }
  }

  return scopedFetch(input, init);
};
