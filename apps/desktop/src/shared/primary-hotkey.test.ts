import { afterEach, describe, expect, it, vi } from "vitest";

const os = vi.hoisted(() => ({ platform: vi.fn() }));

vi.mock("@tauri-apps/plugin-os", () => os);

import { primaryHotkey } from "./primary-hotkey";

describe("primaryHotkey", () => {
  afterEach(() => os.platform.mockReset());

  it("uses ⌘ only on macOS so Ctrl text-editing keys keep working", () => {
    os.platform.mockReturnValue("macos");
    expect(primaryHotkey("k")).toBe("meta+k");
    expect(primaryHotkey("shift+p")).toBe("meta+shift+p");
  });

  it.each(["windows", "linux"])("uses Ctrl on %s", (name) => {
    os.platform.mockReturnValue(name);
    expect(primaryHotkey("p")).toBe("ctrl+p");
  });

  it("falls back to the browser platform outside Tauri", () => {
    os.platform.mockImplementation(() => {
      throw new Error("no os plugin");
    });
    expect(primaryHotkey("k")).toMatch(/^(meta|ctrl)\+k$/);
  });
});
