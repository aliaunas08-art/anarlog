import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

import { filterLicenses } from ".";
import data from "./third-party-licenses.gen.json";

test("ships the app's MIT licence unchanged (D5.1)", () => {
  const license = readFileSync(
    path.resolve(__dirname, "../../../../../LICENSE"),
    "utf8",
  );
  expect(data.app_license).toBe(license);
  expect(data.app_license).toContain("MIT License");
});

test("lists third-party licences for both ecosystems", () => {
  expect(data.cargo.length).toBeGreaterThan(0);
  expect(data.npm.length).toBeGreaterThan(0);
  expect(filterLicenses(data.cargo, "tauri").length).toBeGreaterThan(0);
  expect(
    filterLicenses(
      [{ name: "serde", version: "1", license: "MIT OR Apache-2.0" }],
      "apache",
    ),
  ).toHaveLength(1);
});
