import assert from "node:assert/strict";
import { test } from "node:test";

import { evaluate, verdict } from "./check-licenses.mjs";

test("missing and GPL-family licences fail, permissive ones pass", () => {
  assert.equal(verdict(""), "missing");
  assert.equal(verdict("Unknown"), "missing");
  assert.equal(verdict("UNLICENSED"), "missing");
  assert.equal(verdict("LGPL-3.0"), "copyleft");
  assert.equal(verdict("GPL-2.0-only"), "copyleft");
  assert.equal(verdict("AGPL-3.0"), "copyleft");
  assert.equal(verdict("LGPL"), "copyleft");
  assert.equal(verdict("MIT"), "ok");
  assert.equal(verdict("MIT OR LGPL-3.0"), "ok");
  assert.equal(verdict("MPL-2.0"), "warn");
  assert.equal(verdict("LicenseRef-Modified-MIT"), "warn");
});

test("only an approved allowlist entry for the same licence clears a failure", () => {
  const pkg = {
    ecosystem: "cargo",
    name: "mp3lame-sys",
    version: "0.1.11",
    license: "LGPL-3.0",
  };
  const approved = {
    ecosystem: "cargo",
    name: "mp3lame-sys",
    license: "LGPL-3.0",
    reason: "test",
    approved_by: "owner",
    approved_on: "2026-11-17",
  };
  assert.equal(evaluate([pkg], []).failures.length, 1);
  assert.equal(evaluate([pkg], [approved]).failures.length, 0);
  // Unapproved, wrong licence or wrong version: still fails.
  assert.equal(
    evaluate([pkg], [{ ...approved, approved_by: "" }]).failures.length,
    1,
  );
  assert.equal(
    evaluate([pkg], [{ ...approved, license: "LGPL-2.1" }]).failures.length,
    1,
  );
  assert.equal(
    evaluate([pkg], [{ ...approved, version: "0.2.0" }]).failures.length,
    1,
  );
  // Warnings never fail.
  const mpl = {
    ecosystem: "cargo",
    name: "symphonia",
    version: "0.5.5",
    license: "MPL-2.0",
  };
  assert.deepEqual(evaluate([mpl], []).failures, []);
  assert.equal(evaluate([mpl], []).warnings.length, 1);
});
