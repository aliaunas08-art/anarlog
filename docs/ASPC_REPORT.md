# ASPC final report (2026-11-17)

Branch `prototype`, 50 commits after `18161cd` (audit + rulebook). Nothing is pushed. The rules are in [ASPC_BASE.md](ASPC_BASE.md). Status per rule, file:line references and the test commands are in [ASPC_AUDIT.md](ASPC_AUDIT.md).

## Verification checklist (run 2026-11-17)

| Item | Result | Evidence |
|---|---|---|
| Viewer denied edit, delete, export (A1.4) | TRUE | `viewer_reads_content_only`, `guard_denies_locked_viewer_and_wrong_window` |
| Every command rejects an unauthorised call (A2.2, A2.3) | **FALSE** | 477 commands: 187 Public, 135 guarded and tested, **155 not yet guarded** (2026-11-18) |
| Two-workspace isolation, incl. Ask (C1.4, B4.2) | PARTIAL | Rust reads, search, exports, local-api and Ask are scoped and tested. Webview SQL is fail-closed single-workspace only (see C1 below). |
| Prompt injection: no export, delete or cross-meeting disclosure (B3.4) | TRUE | `injection.test.ts`, `enhance-one-call.test.ts`, template escape tests |
| Audit log detects tampering; no app update/delete path (D1) | TRUE | audit-log 15 + 2, `webview_sql_cannot_reach_the_audit_log` |
| Marker in no log, error or analytics (C4.2) | TRUE | Session, Engine, analytics, tracing and SQL-denial paths. Capture, search and export failures are not covered yet. |
| Delete removes every trace (D2.2) | TRUE | Both purge marker tests (database, WAL, vault, cache, index) |
| Backup restores into a clean library (C5.2) | TRUE | `backup_restores_into_a_clean_library` |
| No secret in repo or fresh DB (C3.1) | TRUE | `scripts/check-secrets`, `--fresh-db`: 0 findings |
| Existing + Engine tests (C7.1, C7.2, C7.5) | PARTIAL | Engine tests all pass. Rust: 629 + 108 + 35 + 30 pass, 0 fail. Vitest: 4422 of 4424 pass; the 2 failures are billing date-text tests that predate this work. |
| `--require-bundle` (C7.7) | NOT RUN | Wired into `desktop_cd.yaml`. Needs a release build. |

All items passed on the first round, so the 3-round escalation rule was never triggered.

## Runtime checks done (dev app, this Mac)

- Phase 1 and 2b: the home screen rendered and meetings loaded (screenshots).
- Phase 4 and 5: no screenshot. macOS kept the window in the Stage Manager strip and refused to capture it. Instead I checked the startup log (no errors), the database (11 meetings intact) and a copy of `audit.db`.
- On first launch, the startup sweep permanently removed 6 meetings that had already been deleted. The old app could only restore these through the 5-second undo, so the decision 5 behaviour applies.
- Permissions were tightened from 755 to 700 and from 644 to 600. FileVault is on.
- The Phase 4 false "permission denied" audit entries no longer appear in this build.

## Open items needing a decision

1. **C1 webview SQL filtering.** The interim fail-closed single-workspace guard is in place. Real filtering needs option (b), scoped Rust commands per table, or option (c), views on a dedicated pool. Decision deferred until a second workspace exists.
2. **Unguarded commands (A2.3): 155 left.** Batch 1 (2026-11-17) guarded the 11 E2EE key and sync-token commands, lock-exempt so background sync keeps working. Batch 2 (2026-11-18) guarded 25 meeting content and transcript commands: 13 fully, 12 without the lock check because they run behind the lock screen. Batch 3 (2026-11-18) guarded the 32 Recording commands: 8 fully, 24 without the lock check (capture, detection, system dictation, end-of-recording saves). File sync, search, calendar, attachment sync, Settings and Actions are next.
3. **Licences (D5): resolved 2026-11-18.** `pnpm licenses:check` passes: 1578 packages, 0 failing, 26 warnings (all MPL-2.0, file-level copyleft, fine to ship unmodified).
   - `mp3lame-encoder` / `mp3lame-sys`: removed 2026-11-17 (recordings are now AAC through macOS AudioToolbox, FLAC on Windows and Linux);
   - `pos` (LGPL): removed with the automatic prose keywords; transcription hints now come from participants, the dictionary and #hashtags;
   - `react-scan` / `react-doctor` (modified MIT that restricts hosted resale): the devtools bar is dev-only, so neither ships; not allowlisted;
   - `khroma`: allowlisted as MIT (its licence file is MIT; only the package metadata is empty);
   - `@giscus/react`, `@splinetool/runtime`: removed earlier with `@lobehub/icons`.
4. **Legal text.**
   - Wording fixed 2026-11-18: sync no longer says "end-to-end encrypted". It says notes, transcripts and summaries are encrypted on this Mac with a Keychain key before upload, that the server still sees metadata such as sizes and timing, that audio isn't synced, and that shared links upload unencrypted. The Anthropic training claim is replaced by a statement that Anthropic's API terms govern storage and use, which this app does not control.
   - **Hard gate: real Privacy Policy and Terms documents must exist before any external user gets a build.** The URLs are placeholders, acceptable in dev builds only.
5. **Gates recorded:**
   - C7.6: a hosted proxy must exist before any build runs on the developer's key;
   - C2.3: SQLCipher must be re-decided before a team or enterprise release.
6. **Backups are not encrypted.** Encrypting them would need a new dependency.

## Future work (deferred, not built)

- **Transcription in its own process.** Since 2026-11-17 the first audio of a recording is on disk before transcription starts, and startup recovery restores it after a crash (`a_crash_mid_transcription_leaves_a_recoverable_recording`). A crash in a speech engine still ends the whole app and stops the recording at that moment. Running the engines in a separate process would let capture continue through such a crash.

## Behaviour changes the user will notice

- **AI:** Haiku only, through the Rust Efficiency Engine. You enter your own Anthropic key, which is kept in the Keychain. The provider picker, the subscription sign-ins and local LLMs are gone.
- **Automations chat:** it can now only answer questions, not make edits.
- **Custom base URLs:** a custom AI base URL needs a native allow dialog.
- **Hooks:** the feature is removed.
- **Analytics, session replay and crash reports:** all off. Anyone who had analytics on must turn it on again.
- **Recording indicator:** the menu-bar icon always shows while recording.
- **Delete:** permanent after the 5-second undo. A quit during the undo window deletes the meeting on the next launch.
- **Retention:** with sync on, a retention setting on one Mac applies on every device.
- **Private flag:** marking a meeting Private does not cancel an existing share. Changes to the flag and MCP calls are not audit-logged yet.

## Hand checks still to do

The step lists are in ASPC_AUDIT.md, in the "runtime check" sections for each phase:

- lock and unlock;
- Engine key entry, enhance, Ask and the AI-down pause;
- composer;
- the Private exclusions;
- delete and quit inside the undo window;
- retention;
- backup and restore;
- consent switches;
- the tray indicator with the tray icon off;
- the About page and "Where your data lives";
- the Keychain prompt in an unsigned build;
- a second Mac with sync on.
