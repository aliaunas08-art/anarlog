import { describe, expect, it } from "vitest";

import {
  hasDarkAppIconVariant,
  normalizeAppIconPreference,
  resolveAppIconName,
  resolveDockIconName,
} from "./icon";

describe("app icon preference", () => {
  it("maps every stored value, old artwork included, to the Timbre icon", () => {
    for (const value of [
      undefined,
      null,
      "unknown",
      "default",
      "stable",
      "dev",
      "staging",
      "anagram",
      "journal",
      "notepad",
      "stone",
      "typewriter-key",
      "walnut",
    ]) {
      expect(normalizeAppIconPreference(value)).toBe("default");
    }
  });

  it("resolves the default icon from the app channel", () => {
    expect(resolveAppIconName("default", "com.hyprnote.stable")).toBe("stable");
    expect(resolveAppIconName("default", "com.hyprnote.nightly")).toBe(
      "staging",
    );
    expect(resolveAppIconName("default", "com.hyprnote.staging")).toBe(
      "staging",
    );
    expect(resolveAppIconName("default", "com.hyprnote.dev")).toBe("dev");
  });

  it("follows the system appearance for the system theme", () => {
    expect(
      resolveDockIconName("anagram", "system", false, "com.hyprnote.dev"),
    ).toBe("anagram");
    expect(
      resolveDockIconName("anagram", "system", true, "com.hyprnote.dev"),
    ).toBe("anagram-dark");
    expect(
      resolveDockIconName("staging", "system", true, "com.hyprnote.stable"),
    ).toBe("staging-dark");
  });

  it("overrides the system appearance with an explicit theme", () => {
    expect(
      resolveDockIconName("anagram", "dark", false, "com.hyprnote.dev"),
    ).toBe("anagram-dark");
    expect(
      resolveDockIconName("anagram", "light", true, "com.hyprnote.dev"),
    ).toBe("anagram");
    expect(
      resolveDockIconName("default", "dark", false, "com.hyprnote.stable"),
    ).toBe("stable-dark");
  });

  it("keeps theme-independent icons unchanged", () => {
    expect(
      resolveDockIconName("journal", "system", true, "com.hyprnote.stable"),
    ).toBe("journal");
    expect(
      resolveDockIconName("stone", "dark", false, "com.hyprnote.stable"),
    ).toBe("stone");
    expect(hasDarkAppIconVariant("anagram")).toBe(true);
    expect(hasDarkAppIconVariant("walnut")).toBe(false);
  });
});
