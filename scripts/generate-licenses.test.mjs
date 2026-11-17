import assert from "node:assert/strict";
import test from "node:test";

import { build, classify } from "./generate-licenses.mjs";

test("classifies SPDX expressions for an MIT app", () => {
  assert.equal(classify("MIT"), "compatible");
  assert.equal(classify("MIT OR Apache-2.0"), "compatible");
  assert.equal(classify("MIT/Apache-2.0"), "compatible");
  assert.equal(classify("apache-2.0"), "compatible");
  assert.equal(classify("(MIT OR Apache-2.0) AND Unicode-3.0"), "compatible");
  assert.equal(classify("Apache-2.0 WITH LLVM-exception"), "compatible");
  assert.equal(classify("MPL-2.0"), "review");
  assert.equal(classify("LGPL-3.0-or-later"), "review");
  assert.equal(classify("GPL-3.0-only"), "incompatible");
  assert.equal(classify("AGPL-3.0"), "incompatible");
  assert.equal(classify("GPL-2.0 OR MIT"), "compatible");
  assert.equal(classify("MIT AND GPL-3.0"), "incompatible");
  assert.equal(classify("Unknown"), "incompatible");
  assert.equal(classify(""), "incompatible");
});

test("flags only packages that need a decision", () => {
  const data = build({
    cargo: [
      { name: "serde", version: "1.0.0", license: "MIT OR Apache-2.0" },
      { name: "gpl-thing", version: "0.1.0", license: "GPL-3.0" },
    ],
    npm: [{ name: "left-pad", version: "1.3.0", license: "WTFPL" }],
    appLicense: "MIT License",
  });
  assert.deepEqual(
    data.flagged.map((pkg) => [pkg.name, pkg.status]),
    [["gpl-thing", "incompatible"]],
  );
  assert.equal(data.app_license, "MIT License");
});
