import { platform } from "@tauri-apps/plugin-os";

function isMacOS(): boolean {
  try {
    return platform() === "macos";
  } catch {
    // Outside Tauri (tests, storybook) the OS plugin is not injected.
    return (
      typeof navigator !== "undefined" &&
      /mac/i.test(navigator.platform || navigator.userAgent)
    );
  }
}

/**
 * The hotkey with the platform's primary modifier: ⌘ on macOS, Ctrl elsewhere.
 * react-hotkeys-hook's `mod` accepts either key on every platform, which on
 * macOS would steal Ctrl+K (kill line), Ctrl+P (previous line) and other
 * Emacs-style text editing keys from inputs.
 *
 * @param keys the rest of the combination, e.g. `"k"` or `"shift+p"`.
 */
export function primaryHotkey(keys: string): string {
  return `${isMacOS() ? "meta" : "ctrl"}+${keys}`;
}
