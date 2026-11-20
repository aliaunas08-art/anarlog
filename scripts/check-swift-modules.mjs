// Guards against Swift symbol collisions between the static Swift libraries
// that swift-rs links into the desktop binary (crates/*/swift-lib,
// plugins/*/swift-lib).
//
// Swift names a private type after its module, its file and the type name. Two
// libraries with the same module name and file name, each with a private
// `StatusPayload`, therefore produce identical symbols. The linker keeps one
// copy of the shared helpers (outlined copy/destroy), so one library ends up
// using the other's memory layout. That crashed the app whenever Apple Speech
// started live transcription (2026-11-17).
//
// Rule: libraries that share a Swift module name must not declare the same
// type name. Giving a library its own target name in Package.swift fixes a
// violation.
//
//   node scripts/check-swift-modules.mjs     # exit 1 on a collision

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** The module name of the library target, from Package.swift's text. */
export function libraryModuleName(manifest) {
  const product = manifest.match(/\.library\([^)]*?targets:\s*\[\s*"([^"]+)"/s);
  if (!product) return null;
  return product[1].replace(/[^A-Za-z0-9_]/g, "_");
}

/** Top-level type names declared in Swift source (nested types are ignored). */
export function declaredTypes(source) {
  const names = new Set();
  const pattern =
    /^(?:@\w+(?:\([^)]*\))?\s+)*(?:(?:public|internal|private|fileprivate|final|open)\s+)*(?:struct|class|enum|actor|protocol)\s+([A-Za-z_]\w*)/gm;
  for (const match of source.matchAll(pattern)) names.add(match[1]);
  return names;
}

/** Collisions: [{ module, type, libraries: [...] }]. */
export function findCollisions(libraries) {
  const seen = new Map();
  for (const lib of libraries) {
    for (const type of lib.types) {
      const key = `${lib.module}\u0000${type}`;
      if (!seen.has(key)) seen.set(key, []);
      seen.get(key).push(lib.dir);
    }
  }
  return [...seen.entries()]
    .filter(([, dirs]) => dirs.length > 1)
    .map(([key, dirs]) => {
      const [module, type] = key.split("\u0000");
      return { module, type, libraries: dirs };
    });
}

function swiftFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) out.push(...swiftFiles(full));
    else if (entry.endsWith(".swift")) out.push(full);
  }
  return out;
}

export function loadLibraries(root = ROOT) {
  const libraries = [];
  for (const parent of ["crates", "plugins"]) {
    for (const name of readdirSync(path.join(root, parent))) {
      const dir = path.join(root, parent, name, "swift-lib");
      const manifest = path.join(dir, "Package.swift");
      if (!existsSync(manifest)) continue;
      const module = libraryModuleName(readFileSync(manifest, "utf8"));
      if (!module) continue;
      const types = new Set();
      for (const file of swiftFiles(path.join(dir, "src"))) {
        for (const type of declaredTypes(readFileSync(file, "utf8"))) {
          types.add(type);
        }
      }
      libraries.push({ dir: path.relative(root, dir), module, types });
    }
  }
  return libraries;
}

function main() {
  const libraries = loadLibraries();
  const collisions = findCollisions(libraries);
  for (const c of collisions) {
    console.error(
      `FAIL: type ${c.type} is declared in module ${c.module} by ${c.libraries.join(" and ")}`,
    );
  }
  console.log(
    `${libraries.length} Swift libraries checked: ${collisions.length} collisions`,
  );
  if (collisions.length > 0) process.exit(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
