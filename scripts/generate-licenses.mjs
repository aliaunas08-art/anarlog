// ASPC D5.1: third-party licences shown in Settings → About → Licences.
//
// Collects the licence of every crate the desktop app links (normal
// dependencies of the `desktop` package for macOS, from `cargo metadata`)
// and every production npm package of `@anlg/desktop` (`pnpm licenses`),
// plus the app's own MIT LICENSE, into a checked-in JSON file. Licences that
// aren't on the MIT-compatible list are listed under `flagged` (D5.3).
//
//   node scripts/generate-licenses.mjs           # rewrite the file
//   node scripts/generate-licenses.mjs --check   # exit 1 if it is stale
//
// Needs the Rust toolchain and pnpm with installed node_modules.
//
// It also writes THIRD_PARTY_NOTICES.md (the same inventory, for people);
// `scripts/check-licenses.mjs` fails on missing or GPL-family licences.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const OUTPUT = path.join(
  ROOT,
  "apps/desktop/src/settings/about/third-party-licenses.gen.json",
);
export const NOTICES = path.join(ROOT, "THIRD_PARTY_NOTICES.md");

// Where a package's metadata misstates its licence, checked against the
// licence file it ships. Keyed by ecosystem and name.
export const LICENSE_CORRECTIONS = {
  // package.json says "MIT"; its LICENSE is a modified MIT that requires
  // written permission for AI training/evaluation use and for selling it as
  // a hosted product. Pulled in by react-scan (devtools bar).
  "npm:react-doctor": "LicenseRef-Modified-MIT",
  "npm:oxlint-plugin-react-doctor": "LicenseRef-Modified-MIT",
};

function corrected(ecosystem, pkg) {
  const fixed = LICENSE_CORRECTIONS[`${ecosystem}:${pkg.name}`];
  return fixed ? { ...pkg, license: fixed } : pkg;
}
const TARGET = "aarch64-apple-darwin";

// SPDX ids that may be combined with, and shipped in, an MIT-licensed app.
// Weak copyleft (MPL-2.0) is allowed for unmodified use but listed apart.
export const MIT_COMPATIBLE = new Set([
  "0BSD",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BlueOak-1.0.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BSL-1.0",
  "CC-BY-4.0",
  "CC0-1.0",
  "CDLA-Permissive-2.0",
  "ISC",
  "MIT",
  "MIT-0",
  "NCSA",
  "Python-2.0",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "Unlicense",
  "WTFPL",
  "Zlib",
]);
export const REVIEW = new Set([
  "MPL-2.0",
  "LGPL",
  "LGPL-2.1",
  "LGPL-3.0",
  "EPL-2.0",
]);
const COMPATIBLE_LOWER = new Set(
  [...MIT_COMPATIBLE].map((id) => id.toLowerCase()),
);
const REVIEW_LOWER = new Set([...REVIEW].map((id) => id.toLowerCase()));

function normalizeId(id) {
  return id
    .trim()
    .replace(/^\(+|\)+$/g, "")
    .replace(/-or-later$|\+$/, "")
    .replace(/-only$/, "");
}

/**
 * "compatible", "review" or "incompatible" for an SPDX expression. OR needs
 * one compatible side, AND needs all. Old-style "MIT/Apache-2.0" is OR.
 */
export function classify(expression) {
  if (!expression || !expression.trim()) return "incompatible";
  const text = expression.replace(/\//g, " OR ");
  const rank = { compatible: 0, review: 1, incompatible: 2 };
  const names = ["compatible", "review", "incompatible"];
  const one = (term) => {
    const id = normalizeId(term).toLowerCase();
    if (COMPATIBLE_LOWER.has(id)) return "compatible";
    if (REVIEW_LOWER.has(id)) return "review";
    return "incompatible";
  };
  const orParts = text.split(/\s+OR\s+/i);
  let best = 2;
  for (const part of orParts) {
    const andParts = part
      .replace(/^\(+|\)+$/g, "")
      .split(/\s+AND\s+/i)
      .map((term) => term.replace(/^\(+|\)+$/g, ""));
    const worst = Math.max(
      ...andParts.map((term) => {
        if (/\s+WITH\s+/i.test(term)) {
          return COMPATIBLE_LOWER.has(term.trim().toLowerCase())
            ? 0
            : rank[one(term.split(/\s+WITH\s+/i)[0])];
        }
        return rank[one(term)];
      }),
    );
    best = Math.min(best, worst);
  }
  return names[best];
}

export function cargoPackages() {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      [
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--filter-platform",
        TARGET,
      ],
      { cwd: ROOT, maxBuffer: 512 * 1024 * 1024, encoding: "utf8" },
    ),
  );
  const byId = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const root = metadata.packages.find((pkg) => pkg.name === "desktop");
  if (!root) throw new Error("desktop package not found");

  const seen = new Set();
  const stack = [root.id];
  while (stack.length > 0) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      // Shipped code only: normal dependencies (kind null).
      if (dep.dep_kinds.some((kind) => kind.kind === null)) {
        stack.push(dep.pkg);
      }
    }
  }
  const out = [];
  for (const id of seen) {
    const pkg = byId.get(id);
    // Workspace crates (no registry source) are the app's own MIT code.
    if (!pkg || pkg.source === null) continue;
    out.push(
      corrected("cargo", {
        name: pkg.name,
        version: pkg.version,
        license: pkg.license ?? (pkg.license_file ? "SEE LICENSE FILE" : ""),
      }),
    );
  }
  return out;
}

export function npmPackages() {
  const grouped = JSON.parse(
    execFileSync(
      "pnpm",
      ["-F", "@anlg/desktop", "licenses", "list", "--prod", "--json"],
      { cwd: ROOT, maxBuffer: 256 * 1024 * 1024, encoding: "utf8" },
    ),
  );
  const out = [];
  for (const [license, packages] of Object.entries(grouped)) {
    for (const pkg of packages) {
      // Workspace packages (`@anlg/*`) are the app's own code.
      if (pkg.name.startsWith("@anlg/")) continue;
      for (const version of pkg.versions) {
        out.push(
          corrected("npm", {
            name: pkg.name,
            version,
            license: pkg.license ?? license,
          }),
        );
      }
    }
  }
  return out;
}

function sortAndDedupe(list) {
  const map = new Map();
  for (const item of list) map.set(`${item.name}@${item.version}`, item);
  return [...map.values()].sort(
    (a, b) =>
      a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
  );
}

export function build({ cargo, npm, appLicense }) {
  const all = [
    ...cargo.map((pkg) => ({ ...pkg, ecosystem: "cargo" })),
    ...npm.map((pkg) => ({ ...pkg, ecosystem: "npm" })),
  ];
  const flagged = all
    .map((pkg) => ({ ...pkg, status: classify(pkg.license) }))
    .filter((pkg) => pkg.status !== "compatible")
    .map(({ name, version, license, ecosystem, status }) => ({
      name,
      version,
      license,
      ecosystem,
      status,
    }));
  return {
    generated_by: "node scripts/generate-licenses.mjs",
    app_license: appLicense,
    cargo: sortAndDedupe(cargo),
    npm: sortAndDedupe(npm),
    flagged: sortAndDedupe(flagged),
  };
}

/** THIRD_PARTY_NOTICES.md: the shipped build's inventory, by licence. */
export function renderNotices(data) {
  const all = [
    ...data.cargo.map((pkg) => ({ ...pkg, ecosystem: "Rust crate" })),
    ...data.npm.map((pkg) => ({ ...pkg, ecosystem: "npm package" })),
  ];
  const byLicense = new Map();
  for (const pkg of all) {
    const key =
      pkg.license && pkg.license !== "Unknown"
        ? pkg.license
        : "(no licence in metadata)";
    if (!byLicense.has(key)) byLicense.set(key, []);
    byLicense.get(key).push(pkg);
  }
  const licenses = [...byLicense.keys()].sort((a, b) => a.localeCompare(b));
  const lines = [
    "# Third-party notices",
    "",
    "Generated by `node scripts/generate-licenses.mjs`; do not edit by hand.",
    "",
    "This is the inventory of third-party code in the shipped desktop app:",
    "every crate the macOS build links (normal dependencies of `desktop`,",
    "target `aarch64-apple-darwin`) and every production npm package of",
    "`@anlg/desktop`. Licences come from package metadata, except the",
    "corrections listed in `LICENSE_CORRECTIONS`. Licence texts ship with",
    "each package; Settings → About → Licences lists the same data.",
    "",
    `Totals: ${data.cargo.length} Rust crates, ${data.npm.length} npm packages.`,
    "",
    "## Needs review",
    "",
    "Licences that are not plainly MIT-compatible (`node scripts/check-licenses.mjs`",
    "fails on missing and GPL-family licences unless they are in",
    "`scripts/license-allowlist.json`):",
    "",
    "| Package | Version | Ecosystem | Licence | Status |",
    "|---|---|---|---|---|",
    ...data.flagged.map(
      (pkg) =>
        `| ${pkg.name} | ${pkg.version} | ${pkg.ecosystem} | ${pkg.license || "(none)"} | ${pkg.status} |`,
    ),
    "",
    "## By licence",
    "",
  ];
  for (const license of licenses) {
    const pkgs = byLicense.get(license);
    lines.push(`### ${license} (${pkgs.length})`, "");
    for (const pkg of pkgs) {
      lines.push(`- ${pkg.name} ${pkg.version} (${pkg.ecosystem})`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

function main() {
  const check = process.argv.includes("--check");
  const data = build({
    cargo: cargoPackages(),
    npm: npmPackages(),
    appLicense: readFileSync(path.join(ROOT, "LICENSE"), "utf8"),
  });
  const text = `${JSON.stringify(data, null, 2)}\n`;
  const notices = renderNotices(data);
  if (check) {
    const read = (file) => {
      try {
        return readFileSync(file, "utf8");
      } catch {
        return "";
      }
    };
    for (const [file, expected] of [
      [OUTPUT, text],
      [NOTICES, notices],
    ]) {
      if (read(file) !== expected) {
        console.error(
          `${path.relative(ROOT, file)} is stale; run node scripts/generate-licenses.mjs`,
        );
        process.exit(1);
      }
    }
    console.log("licences up to date");
    return;
  }
  writeFileSync(OUTPUT, text);
  writeFileSync(NOTICES, notices);
  console.log(
    `wrote ${data.cargo.length} crates and ${data.npm.length} npm packages; ${data.flagged.length} flagged`,
  );
  for (const pkg of data.flagged) {
    console.log(
      `  ${pkg.status}: ${pkg.ecosystem} ${pkg.name}@${pkg.version} (${pkg.license || "no licence"})`,
    );
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
