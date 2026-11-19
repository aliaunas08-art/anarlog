import assert from "node:assert/strict";
import { test } from "node:test";

import {
  declaredTypes,
  findCollisions,
  libraryModuleName,
} from "./check-swift-modules.mjs";

test("reads the library target's module name", () => {
  const manifest = `products: [ .library( name: "x-swift", type: .static, targets: ["swift-lib"]) ]`;
  assert.equal(libraryModuleName(manifest), "swift_lib");
  assert.equal(
    libraryModuleName(`.library(name: "y", targets: ["AppleSpeechBridge"])`),
    "AppleSpeechBridge",
  );
});

test("finds top-level types, including private ones, and skips nested ones", () => {
  const types = declaredTypes(
    [
      "private struct StatusPayload: Codable {",
      "  struct Nested {}",
      "}",
      "@available(macOS 26.0, *)",
      "private final class LiveSession {}",
      "enum RustBridge {}",
      "actor Bridge {}",
    ].join("\n"),
  );
  assert.deepEqual([...types].sort(), [
    "Bridge",
    "LiveSession",
    "RustBridge",
    "StatusPayload",
  ]);
});

test("the 2026-11-17 crash: same module, same private type", () => {
  const collisions = findCollisions([
    { dir: "a", module: "swift_lib", types: new Set(["StatusPayload"]) },
    { dir: "b", module: "swift_lib", types: new Set(["StatusPayload"]) },
  ]);
  assert.deepEqual(collisions, [
    { module: "swift_lib", type: "StatusPayload", libraries: ["a", "b"] },
  ]);
});

test("different module names do not collide", () => {
  const collisions = findCollisions([
    {
      dir: "a",
      module: "AppleSpeechBridge",
      types: new Set(["StatusPayload"]),
    },
    { dir: "b", module: "SoniqoBridge", types: new Set(["StatusPayload"]) },
  ]);
  assert.deepEqual(collisions, []);
});
