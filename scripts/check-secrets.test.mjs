import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { findSecrets, scan, scanDatabaseFiles } from "./check-secrets.mjs";

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

test("a key stored in a database file fails, even inside binary pages", async () => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "check-secrets-db-"));
  try {
    const file = path.join(dir, "app.db");
    const page = Buffer.concat([
      Buffer.from("SQLite format 3\0"),
      Buffer.alloc(64),
      Buffer.from(realLooking),
      Buffer.alloc(64),
    ]);
    await writeFile(file, page);
    await writeFile(`${file}-wal`, Buffer.alloc(32));
    const findings = scanDatabaseFiles([file, `${file}-wal`, `${file}-shm`]);
    assert.equal(findings.length, 1);
    assert.equal(findings[0].name, "Anthropic key");
    // A fake-looking key is not allowed in a database either.
    await writeFile(file, Buffer.from("sk-ant-test-0000000000000000"));
    assert.equal(scanDatabaseFiles([file]).length, 1);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
