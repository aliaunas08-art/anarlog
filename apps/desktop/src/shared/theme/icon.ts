import { resolveIsDarkMode, type ThemePreference } from "./resolve";

export type AppIconPreference =
  | "default"
  | "stable"
  | "anagram"
  | "dev"
  | "staging"
  | "journal"
  | "notepad"
  | "stone"
  | "typewriter-key"
  | "walnut";

/**
 * Only the Timbre icon is offered now. The other names (the old Anarlog
 * artwork: Anagram, Field Journal, Stone…) stay in the type so old builds'
 * stored values still parse, but any stored choice falls back to the
 * default, so the Dock never shows the previous brand.
 */
export function normalizeAppIconPreference(
  _value: string | null | undefined,
): AppIconPreference {
  return "default";
}

export function resolveAppIconName(
  icon: AppIconPreference,
  appIdentifier: string,
): Exclude<AppIconPreference, "default"> {
  if (icon !== "default") {
    return icon;
  }
  if (appIdentifier.endsWith(".dev")) {
    return "dev";
  }
  if (
    appIdentifier.endsWith(".staging") ||
    appIdentifier.endsWith(".nightly")
  ) {
    return "staging";
  }
  return "stable";
}

/** `systemIsDark` is the Dock's appearance, used when the theme follows the system. */
export function resolveDockIconName(
  icon: AppIconPreference,
  theme: ThemePreference,
  systemIsDark: boolean,
  appIdentifier: string,
): string {
  const name = resolveAppIconName(icon, appIdentifier);
  return hasDarkAppIconVariant(name) && resolveIsDarkMode(theme, systemIsDark)
    ? `${name}-dark`
    : name;
}

export function hasDarkAppIconVariant(
  name: Exclude<AppIconPreference, "default">,
): boolean {
  return (
    name === "stable" ||
    name === "anagram" ||
    name === "dev" ||
    name === "staging"
  );
}
