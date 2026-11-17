import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

import { describe, expect, it } from "vitest";

// ASPC C1.1: every raw-SQL insert into a content table from the frontend
// names `workspace_id` and never writes it as a literal ''. Drizzle inserts
// get the local ids from `$defaultFn` (packages/db/src/schema.ts). Keep this
// list in step with WORKSPACE_CONTENT_TABLES in
// crates/db-app/src/content_identity.rs.
const CONTENT_TABLES = [
  "action_items",
  "chat_groups",
  "chat_messages",
  "daily_notes",
  "entity_mentions",
  "folder_attachments",
  "folders",
  "humans",
  "organizations",
  "session_attachments",
  "session_document_versions",
  "session_documents",
  "session_participants",
  "session_proposals",
  "session_tags",
  "sessions",
  "synced_preferences",
  "tags",
  "transcripts",
  "voiceprint_candidates",
  "voiceprint_exemplars",
];

// Tables whose own insert trigger stamps the local workspace when the
// column is left out (migrations `e2ee_workspace_stamp_*`).
const STAMPED_BY_TRIGGER = new Set([
  "tags",
  "session_tags",
  "folders",
  "daily_notes",
]);

const SRC = join(__dirname, "..");

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    if (!/\.(ts|tsx)$/.test(name) || /\.test\.(ts|tsx)$/.test(name)) return [];
    return [path];
  });
}

const INSERT = new RegExp(
  `INSERT\\s+(?:OR\\s+\\w+\\s+)?INTO\\s+(${CONTENT_TABLES.join("|")})\\s*\\(([^)]*)\\)\\s*(VALUES\\s*\\(([^)]*)\\))?`,
  "g",
);

describe("frontend content inserts (ASPC C1.1)", () => {
  const inserts = sourceFiles(SRC).flatMap((file) =>
    [...readFileSync(file, "utf8").matchAll(INSERT)].map((match) => ({
      where: `${relative(SRC, file)} ${match[1]}`,
      table: match[1],
      columns: match[2].split(",").map((column) => column.trim()),
      values: match[4]?.split(",").map((value) => value.trim()) ?? null,
    })),
  );

  it("finds the inserts it checks", () => {
    expect(inserts.length).toBeGreaterThan(10);
  });

  it("always names workspace_id, unless a trigger stamps it", () => {
    const missing = inserts
      .filter(
        (insert) =>
          !insert.columns.includes("workspace_id") &&
          !STAMPED_BY_TRIGGER.has(insert.table),
      )
      .map((insert) => insert.where);
    expect(missing).toEqual([]);
  });

  it("never writes workspace_id as a literal ''", () => {
    const literal = inserts
      .filter((insert) => {
        const index = insert.columns.indexOf("workspace_id");
        return index >= 0 && insert.values?.[index] === "''";
      })
      .map((insert) => insert.where);
    expect(literal).toEqual([]);
  });
});
