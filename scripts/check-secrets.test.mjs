import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { findSecrets, scan } from "./check-secrets.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const realLooking = ["sk-ant-api03-", "Q7v9xLm2RtP4wZk8NcB3hJ6d"].join("");

test("a key-like string in app code fails", () => {
  const findings = findSecrets(`const key = "${realLooking}";`, {
    file: "apps/desktop/src/ai/engine/model.ts",
    inBundle: false,
  });
  assert.equal(findings.length, 1);
  assert.equal(findings[0].name, "Anthropic key");
  assert.ok(!findings[0].value.includes("Q7v9"), "the value is truncated");
});

test("obviously fake keys pass only in test files, never in a bundle", () => {
  const fake = 'const FAKE_KEY = "sk-ant-test-0000000000000000";';
  assert.equal(
    findSecrets(fake, {
      file: "crates/ai-engine/src/tests.rs",
      inBundle: false,
    }).length,
    0,
  );
  assert.equal(
    findSecrets(fake, { file: "crates/ai-engine/src/lib.rs", inBundle: false })
      .length,
    1,
  );
  assert.equal(
    findSecrets(fake, { file: "dist/assets/index.js", inBundle: true }).length,
    1,
  );
});

test("a built bundle with an embedded key fails", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "check-secrets-"));
  try {
    await mkdir(path.join(dir, "assets"));
    await writeFile(path.join(dir, "assets", "index.js"), `x="${realLooking}"`);
    const { findings, bundleScanned } = scan({ root, bundle: dir });
    assert.ok(bundleScanned);
    assert.ok(findings.some((f) => f.file.endsWith("index.js")));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("the repository sources hold no key", () => {
  const { findings } = scan({ root, bundle: undefined });
  assert.deepEqual(findings, []);
});
