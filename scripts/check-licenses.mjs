// Licence gate for the shipped desktop build (ASPC D5).
//
// Collects the same inventory as `generate-licenses.mjs` (crates the macOS
// build links, production npm packages of `@anlg/desktop`) and fails when a
// package has
//   - no licence in its metadata, or
//   - an AGPL, GPL or LGPL licence with no permissive alternative,
// unless that package is in `scripts/license-allowlist.json`, which only the
// project owner edits. Other licences that need a look (MPL-2.0, custom
// LicenseRef-* licences) are printed as warnings.
//
//   node scripts/check-licenses.mjs            # exit 1 on a violation
//   node scripts/check-licenses.mjs --json     # machine-readable result
//
// Needs the Rust toolchain and pnpm with installed node_modules.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { cargoPackages, classify, npmPackages } from "./generate-licenses.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const ALLOWLIST = path.join(ROOT, "scripts/license-allowlist.json");

const GPL_FAMILY = /\b(A?GPL|LGPL)\b/i;

/** "missing", "copyleft", "warn" or "ok" for one package's licence. */
export function verdict(license) {
  const text = (license ?? "").trim();
  if (!text || /^unknown$/i.test(text) || /^unlicensed$/i.test(text)) {
    return "missing";
  }
  // An OR expression with a permissive side is fine.
  if (classify(text) === "compatible") return "ok";
  if (GPL_FAMILY.test(text)) return "copyleft";
  return "warn";
}

/**
 * Allowlist entries: { ecosystem, name, version?, license, reason,
 * approved_by, approved_on }. An entry matches the ecosystem and name, the
 * version if given, and must name the licence it approves, so a licence
 * change needs a new approval.
 */
export function isAllowed(pkg, allowlist) {
  return allowlist.some(
    (entry) =>
      entry.ecosystem === pkg.ecosystem &&
      entry.name === pkg.name &&
      (!entry.version || entry.version === pkg.version) &&
      entry.license === (pkg.license ?? "") &&
      Boolean(entry.approved_by),
  );
}

export function evaluate(packages, allowlist) {
  const failures = [];
  const warnings = [];
  for (const pkg of packages) {
    const result = verdict(pkg.license);
    if (result === "ok") continue;
    if (result === "warn") {
      warnings.push({ ...pkg, reason: "needs review" });
      continue;
    }
    if (isAllowed(pkg, allowlist)) continue;
    failures.push({
      ...pkg,
      reason: result === "missing" ? "no licence" : "GPL-family licence",
    });
  }
  return { failures, warnings };
}

function loadAllowlist() {
  const data = JSON.parse(readFileSync(ALLOWLIST, "utf8"));
  if (!Array.isArray(data.allow)) {
    throw new Error(`${ALLOWLIST}: expected { "allow": [...] }`);
  }
  return data.allow;
}

function main() {
  const packages = [
    ...cargoPackages().map((pkg) => ({ ...pkg, ecosystem: "cargo" })),
    ...npmPackages().map((pkg) => ({ ...pkg, ecosystem: "npm" })),
  ];
  const { failures, warnings } = evaluate(packages, loadAllowlist());
  if (process.argv.includes("--json")) {
    console.log(JSON.stringify({ failures, warnings }, null, 2));
  } else {
    for (const pkg of warnings) {
      console.log(
        `warning: ${pkg.ecosystem} ${pkg.name}@${pkg.version} (${pkg.license}) needs review`,
      );
    }
    for (const pkg of failures) {
      console.error(
        `FAIL: ${pkg.ecosystem} ${pkg.name}@${pkg.version} (${pkg.license || "no licence"}): ${pkg.reason}`,
      );
    }
    console.log(
      `${packages.length} packages checked: ${failures.length} failing, ${warnings.length} warnings`,
    );
  }
  if (failures.length > 0) process.exit(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
