// Hard rule (ASPC C7 / C3.1): no developer key is ever embedded in, or
// shipped with, a distributed build.
//
// Scans the tracked sources (Rust and the desktop frontend) and the built
// frontend bundle for API-key-like strings. Exits 1 on any finding.
//
//   node scripts/check-secrets.mjs                 # sources + apps/desktop/dist if built
//   node scripts/check-secrets.mjs --bundle <dir>  # a specific built bundle
//   node scripts/check-secrets.mjs --require-bundle  # fail if no bundle exists (release CI)
//
// In sources, obviously fake keys in test code are allowed: the match must
// sit in a test file and contain a marker such as "test" or "fake". In a
// built bundle nothing is allowed. Test-only signing keys (PEM blocks in
// the API crates' tests) are not API keys and are out of scope here.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const KEY_PATTERNS = [
  { name: "Anthropic key", re: /sk-ant-[A-Za-z0-9_-]{16,}/g },
  { name: "OpenAI-style key", re: /sk-(?!ant-)(?:proj-)?[A-Za-z0-9_-]{32,}/g },
  { name: "AWS access key", re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g },
  { name: "Google API key", re: /\bAIza[0-9A-Za-z_-]{35}\b/g },
];

const FAKE_MARKERS = /test|fake|example|dummy|placeholder|0000|xxxx/i;

export function isTestPath(file) {
  return (
    /(^|\/)tests?\//.test(file) ||
    /(^|\/)tests\.rs$/.test(file) ||
    /\.test\.[cm]?[jt]sx?$/.test(file) ||
    /(^|\/)fixtures?\//.test(file) ||
    /(^|\/)testing\.ts$/.test(file)
  );
}

/** Findings in one text. `source` decides whether fake test keys pass. */
export function findSecrets(text, { file, inBundle }) {
  const findings = [];
  for (const { name, re } of KEY_PATTERNS) {
    for (const match of text.matchAll(re)) {
      const value = match[0];
      const allowed = !inBundle && isTestPath(file) && FAKE_MARKERS.test(value);
      if (!allowed) {
        findings.push({ file, name, value: `${value.slice(0, 10)}…` });
      }
    }
  }
  return findings;
}

const SOURCE_PATTERNS = [
  "*.rs",
  "apps/desktop/src/**",
  "apps/desktop/src-tauri/**",
  "plugins/**",
  "*.env",
  "*.env.*",
];

function trackedSources(root) {
  const out = execFileSync(
    "git",
    ["ls-files", "-z", "--", ...SOURCE_PATTERNS],
    {
      cwd: root,
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  return out.split("\0").filter(Boolean);
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

function isText(buffer) {
  return !buffer.subarray(0, 8000).includes(0);
}

export function scan({ root, bundle, requireBundle }) {
  const findings = [];
  for (const file of trackedSources(root)) {
    const full = path.join(root, file);
    if (!existsSync(full)) continue;
    const buffer = readFileSync(full);
    if (!isText(buffer)) continue;
    findings.push(
      ...findSecrets(buffer.toString("utf8"), { file, inBundle: false }),
    );
  }

  let bundleScanned = false;
  if (bundle && existsSync(bundle)) {
    bundleScanned = true;
    for (const full of walk(bundle)) {
      const buffer = readFileSync(full);
      if (!isText(buffer)) continue;
      findings.push(
        ...findSecrets(buffer.toString("utf8"), {
          file: path.relative(root, full),
          inBundle: true,
        }),
      );
    }
  } else if (requireBundle) {
    findings.push({
      file: bundle ?? "(none)",
      name: "bundle missing",
      value: "",
    });
  }
  return { findings, bundleScanned };
}

function main(argv) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const bundleIndex = argv.indexOf("--bundle");
  const bundle =
    bundleIndex >= 0
      ? path.resolve(argv[bundleIndex + 1])
      : path.join(root, "apps/desktop/dist");
  const { findings, bundleScanned } = scan({
    root,
    bundle,
    requireBundle: argv.includes("--require-bundle"),
  });
  for (const finding of findings) {
    console.error(`${finding.file}: ${finding.name} ${finding.value}`);
  }
  console.log(
    `check-secrets: sources scanned; bundle ${bundleScanned ? "scanned" : "not built, skipped"}; ${findings.length} finding(s)`,
  );
  return findings.length === 0 ? 0 : 1;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(main(process.argv.slice(2)));
}
