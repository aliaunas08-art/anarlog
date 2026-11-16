# ASPC audit: Access, Security, Protection, Compliance

Audit of the `prototype` branch of this fork, done on 2026-11-16, before any ASPC code changes.
Rules being audited against: [ASPC_BASE.md](ASPC_BASE.md). Paths are relative to the repo root.

**How to read this:**

- Every finding cites `file:line`.
- **[V]** means I re-read the code myself to confirm it.
- Unmarked findings come from four read-only audit passes; I spot-checked them but didn't re-read every line.
- **GUESS** means it's inferred and wasn't seen in code.
- Statuses: EXISTS, PARTIAL, MISSING, or N/A (with the reason).

## Summary

Bottom line: the app's _data model_ is further along than the brief assumes, but its _trust boundary_ is much weaker.

**Already in the data model:**

- workspaces;
- an owner on every content row;
- a Touch ID app lock;
- a per-note lock;
- secrets in the Keychain;
- prompt guardrails on Ask.

**The trust-boundary problem:** the webview is trusted completely, and the webview runs with almost no limits:

- there's no content security policy;
- every window gets every permission;
- the frontend sends raw SQL to the database;
- one command writes files anywhere;
- the settings "hooks" feature runs shell commands.

So any script injected into the webview (for example by a bug in rendering a transcript or calendar invite) could:

- read every meeting;
- read the API keys the webview can see;
- send data to any host;
- run programs on the Mac.

Most of the ASPC work is closing that boundary.

**Three premises in the brief don't match this codebase:**

1. **There is no "Efficiency Engine".** No code or docs by that name exist, and there's no locked Haiku model.
   - AI calls go straight from the webview to whichever provider and model the user picked, through one shared factory: `apps/desktop/src/ai/hooks/useLLMConnection.ts:301-329`.
   - The only quota is an optional daily Ask limit on the hosted key: `apps/desktop/src/ai/usage.ts:220`, setting at `apps/desktop/src/settings/schema.ts:334-339` [V].
   - C7 and verification item 10 can't be tested until the Engine is built. See decision 1.
2. **No "existing security hardening checklist" was found** in the repo or my notes. ASPC_BASE.md §B1 now _is_ that checklist.
3. **"P1" isn't a section of the spec.** I've read it as **C1** (Protection, item 1: tenant isolation), because it's the third design the brief asks for alongside A1 and A2.

## 1. Data stores

**Base folder:** `~/Library/Application Support/anarlog`. Dev, staging and nightly builds use the bundle id instead (dev: `com.hyprnote.dev`). See `crates/storage/src/global.rs:3-31`.

**Database:** one SQLite file, `app.db`, in WAL mode.

- Filename at `apps/desktop/src-tauri/src/db.rs:5`; WAL set at `plugins/db/src/runtime/open.rs:43-49`.
- The driver is `sqlx` 0.9.0-alpha.1 with `sqlite-unbundled`, so it links the system libsqlite3. See `crates/db-app/Cargo.toml:17` and `Cargo.toml:405` [V].

| Store                                                                                                           | Holds                                                                                                   | Readable by                                                                                                                         | Encrypted at rest                       |
| --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| `sessions` (`crates/db-app/migrations/20260710223922_canonical_data_model.sql:34`)                              | meetings, title, calendar `event_json`                                                                  | the webview via raw SQL, CLI/MCP, local API                                                                                         | No                                      |
| `session_documents` (:59), `session_document_versions`                                                          | notes, summaries, meeting chat, previous versions                                                       | same                                                                                                                                | No                                      |
| `transcripts` (:78), `transcript_live_state` / `_deltas`                                                        | full transcript words                                                                                   | same                                                                                                                                | No                                      |
| `session_participants` (:99), `humans`, `organizations`                                                         | names, emails, phone, LinkedIn                                                                          | same                                                                                                                                | No                                      |
| `action_items`, `entity_mentions`, `tags`, `daily_notes`, `chat_groups` / `chat_messages`                       | derived content, global AI chats                                                                        | same                                                                                                                                | No                                      |
| `voiceprint_exemplars`, `voiceprint_candidates`                                                                 | voiceprint metadata; the embeddings themselves are in the Keychain                                      | native code only for the embeddings (`plugins/store2/src/commands.rs:7`) [V]                                                        | Keychain                                |
| `session_participant_consent`, `session_disclosure_attempts`                                                    | consent evidence, never synced                                                                          | same as other tables                                                                                                                | No                                      |
| `webhook_endpoints.secret` (`crates/db-app/migrations/20260728090000_local_api.sql:19-35`)                      | webhook signing secret                                                                                  | same                                                                                                                                | **No: secret in plaintext**             |
| `e2ee_*` (about 17 tables)                                                                                      | ciphertext copies for CloudSync                                                                         | same                                                                                                                                | Yes (XChaCha20-Poly1305, `crates/e2ee`) |
| `app_settings`                                                                                                  | settings; legacy plaintext API keys are migrated out (`apps/desktop/src/settings/providers.ts:467-561`) | same                                                                                                                                | No                                      |
| `sessions/<id>/audio.*`, `audio-recovery/*.part`, `attachments/` (`crates/fs-sync-core/src/audio/mod.rs:18-27`) | audio and attachments                                                                                   | webview via the asset protocol (scope `**/*`), Finder, any process running as the user                                              | No                                      |
| `search_index/` (Tantivy, `plugins/tantivy/src/ext.rs:91-127`)                                                  | full note, summary and transcript text per session                                                      | webview via `tantivy:search`                                                                                                        | No                                      |
| `app_cache_dir/audio-peaks/<id>.json`                                                                           | waveform cache                                                                                          | webview                                                                                                                             | No                                      |
| `auth.json` (`plugins/auth/src/migrate.rs:8`)                                                                   | Supabase access and refresh tokens (hosted sign-in only)                                                | webview via `auth:get_item`                                                                                                         | **No: plaintext file on macOS**         |
| `settings.json`, `store.json` (`crates/storage/src/vault/path.rs:6`, `plugins/store2/src/ext.rs:21`)            | settings, including `hooks` commands                                                                    | webview                                                                                                                             | No                                      |
| `~/Library/Logs/<bundle-id>/app.log` (`apps/desktop/src-tauri/src/lib.rs:700-708`)                              | logs, redacted (`plugins/tracing/src/redaction.rs`)                                                     | user, webview via `tracing:log_content`                                                                                             | No                                      |
| Keychain `<id>.secure-store`                                                                                    | LLM and STT keys, Claude/ChatGPT subscription tokens, e2ee and voiceprint secrets                       | webview via `store2:get_secret`, except the `e2ee:` and `voice_enrollment:` prefixes (`plugins/store2/src/commands.rs:7,26-37`) [V] | Keychain                                |
| Keychain `<id>.accounts`                                                                                        | Google and Microsoft refresh tokens (`plugins/accounts/src/lib.rs:91`) [V]                              | native code only; no command returns a token                                                                                        | Keychain                                |

**Workspace and owner columns already exist:**

- Nearly every content table has `workspace_id` and `owner_user_id`, both defaulting to `''`. See `sessions` at `canonical_data_model.sql:36-37` [V].
- There are `workspaces` and `workspace_memberships(role)` tables (`20260716120000_personal_workspaces.sql:1-20`).
- Signed out, rows carry `''` or the all-zeros user id (`apps/desktop/src/shared/utils.ts:74`).

**Nothing local is encrypted by the app:**

- There's no SQLCipher, `PRAGMA key` or `rekey` anywhere.
- `crates/e2ee` is used only for sync and cloud backup.
- Protection at rest therefore depends entirely on FileVault. That's a GUESS about the user's Mac, and the app doesn't check it.

## 2. Entry points

**Trust model today:**

- There's one capability file, `apps/desktop/src-tauri/capabilities/default.json`.
- Its `"windows": ["*"]` grants `<plugin>:default` to every window, covering main, composer, `note-<id>`, the floating bar, the dictation overlay and live captions (`:5-7`) [V].
- No command checks who is calling beyond Tauri's ACL. Locks are enforced only in React: `apps/desktop/src/lock/gate.tsx:39-112`, `apps/desktop/src/lock/notes.ts`.
- Plugins: about 50 registered, with roughly 450 commands (`apps/desktop/src-tauri/src/lib.rs:267-403`).
- App-level commands: 20 (`lib.rs:756-775`). Their `build.rs` has no app manifest, so they're GUESS-ungated by the ACL.

**No check beyond the ACL, and dangerous (all of these are A2 failures):**

| Entry point                                                                                     | Problem                                                                                                                                                                                                                                          |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `db:execute`, `execute_proxy`, `execute_transaction` (`plugins/db/src/commands.rs:110-150`) [V] | ~~Raw SQL from the webview with no authorizer, so any read or write is possible, including `ATTACH`.~~ **PARTIAL (Phase 1):** now runs under the webview SQL authorizer, see Phase 1 → A2.4. Row-level and per-`DataType` mapping still missing. |
| `fs2:write_text_file` (`plugins/fs2/src/commands.rs:27-32`) [V]                                 | Writes any path.                                                                                                                                                                                                                                 |
| `export:export` (`plugins/export/src/commands.rs:7-15`)                                         | Writes a PDF to any path.                                                                                                                                                                                                                        |
| ~~`settings:save` + `hooks:run_event_hooks` (`crates/hooks/src/runner.rs:48-65`)~~              | **FIXED (Phase 1):** the hooks feature was removed; see Phase 1 → Hooks.                                                                                                                                                                         |
| `opener2:open_path` / `open_url` (`plugins/opener2/src/ext.rs:10-18`)                           | `open_with` isn't validated. GUESS: chained with `write_text_file`, this could run code.                                                                                                                                                         |
| `get_env(key)` (`apps/desktop/src-tauri/src/commands.rs:45-47`) [V]                             | Returns any environment variable.                                                                                                                                                                                                                |
| `store2:get_secret`, `auth:get_item`                                                            | Return LLM/STT keys and sign-in tokens to JS.                                                                                                                                                                                                    |
| `local-api:create_webhook` (`plugins/local-api/src/dispatch.rs:123-151`)                        | POSTs transcript text to any http(s) URL.                                                                                                                                                                                                        |
| `http:default` scope (`capabilities/default.json:99-116`) [V]                                   | Any host, including localhost and the LAN.                                                                                                                                                                                                       |
| asset protocol scope `**/*` (`apps/desktop/src-tauri/tauri.conf.json:18-23`) [V]                | The webview can read any file.                                                                                                                                                                                                                   |
| `importer:read_text_files` (`plugins/importer/src/commands.rs:69-94`)                           | Reads any path, checked only by extension.                                                                                                                                                                                                       |
| `fs-sync:audio_import` (`plugins/fs-sync/src/commands.rs:314-326`)                              | The source path isn't checked.                                                                                                                                                                                                                   |
| `template:render_custom` (`plugins/template/src/commands.rs:120-125`)                           | Renders any MiniJinja template. It can't run code but has no fuel limit (CPU denial of service).                                                                                                                                                 |
| `detect:send_meeting_chat_message` (`plugins/detect/src/commands.rs:140`)                       | Posts into the live meeting's chat.                                                                                                                                                                                                              |

**Non-IPC entry points:**

- **Local STT server** (`plugins/local-stt/src/server/internal.rs:23-72`): `127.0.0.1:0` with CORS `Any`, no auth, no WebSocket Origin check. It transcribes any audio sent to it but doesn't read stored data.
- **deeplink2 OAuth callback** (`plugins/deeplink2/src/server/mod.rs:207-288`): no state check in Rust. The frontend checks state, but the ChatGPT flow accepts a missing state (`apps/desktop/src/settings/ai/llm/subscriptions/oauth.ts:254-258`).
- **Deep links:** `anarlog://` etc. (`plugins/deeplink2/src/lib.rs:59-130`). `billing/refresh` and `onboarding-demo/complete` act with no check, but touch no data.
- **CLI / MCP** (`apps/cli/src/db.rs:6-25`, `apps/cli/src/mcp.rs:499`): opens `app.db` directly. The only protection is OS file permissions. Ignores the note lock.
- **OAuth loopbacks:** `crates/local-accounts/src/loopback.rs` and `plugins/importer/src/connected_mcp.rs:158` check state and use PKCE. OK.
- **Relay** (`plugins/relay`): debug builds only. Checks Host, Origin and a cookie. OK.
- **File drag-drop:** disabled (`plugins/windows/src/window/v1.rs:182`).

## 3. Secrets and tokens

| Secret                                                    | Where today                                                                                                 | Target                                        | Status                                                                                                                    |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Anthropic, OpenAI and other LLM/STT BYOK keys             | Keychain `secure-store`, scope `ai-provider-api-keys` (`apps/desktop/src/settings/providers.ts:24,141-145`) | Keychain, not readable by the webview         | PARTIAL: the webview reads them, and they're held in JS memory                                                            |
| Legacy plaintext keys in `app_settings`                   | migrated out at startup with `secure_delete` (`providers.ts:467-561`)                                       | gone                                          | PARTIAL: still used as a fallback if the Keychain is empty (`providers.ts:64`); GUESS: old WAL/`.bak` pages may keep them |
| Claude/ChatGPT subscription OAuth                         | Keychain, as the provider `api_key` (`apps/desktop/src/settings/ai/llm/subscriptions/access.ts:14,63-65`)   | Keychain                                      | PARTIAL: same as above                                                                                                    |
| Google/Microsoft calendar refresh tokens                  | Keychain `<id>.accounts`, native only (`crates/local-accounts/src/store.rs`) [V]                            | Keychain                                      | EXISTS                                                                                                                    |
| Google OAuth "client secret"                              | env, or `option_env!` into the binary (`crates/local-accounts/src/provider.rs:64-88`) [V]                   | public by Google's design for desktop clients | EXISTS (documented as non-confidential)                                                                                   |
| Supabase session                                          | `auth.json` plaintext on macOS (`plugins/auth/src/migrate.rs:8`)                                            | Keychain                                      | MISSING (only affects hosted sign-in)                                                                                     |
| Webhook secrets                                           | `webhook_endpoints.secret` plaintext in SQLite                                                              | Keychain                                      | MISSING                                                                                                                   |
| E2EE recovery and device keys, voiceprint                 | Keychain, native only (`plugins/db/src/commands.rs:22-29`)                                                  | Keychain                                      | EXISTS                                                                                                                    |
| Developer proxy key (the brief's "Efficiency Engine" key) | does not exist                                                                                              | Keychain, or held server-side by a proxy      | N/A until decision 1                                                                                                      |

**Committed-secret scan** (`git grep` on tracked files):

- No live keys found.
- `enterprise/control-plane/.env.sample` holds placeholders only.
- `BEGIN PRIVATE KEY` and `sk-ant-oat01-token` appear only in test fixtures.
- `supabase/tests/000-setup-tests-hooks.sql:21` holds a Supabase **anon** JWT for a real project ref. It's publishable by design, but confirm it's upstream's and intended.
- Some STT adapters put the key in the URL query string, which providers can log: `crates/owhisper-client/src/adapter/modulate.rs:97`, `wisprflow/live.rs:60`, `google_generative_ai/live.rs:209`.

## 4. AI calls

- **One factory:** every model is created by `createLanguageModel` → `createProviderModel` (`apps/desktop/src/ai/hooks/useLLMConnection.ts:301-494`) and wrapped with usage tracking (`:312-328`).
  - All tasks (enhance, title, Ask, classifier) use the same model the user picked (`:123-171`).
  - There's no model lock on the client.
  - The hosted proxy has a server-side allowlist (`crates/llm-proxy/src/model.rs:11-29`) but no rate limit.
- **Calls:**
  - enhance: `apps/desktop/src/store/zustand/ai-task/task-configs/enhance-workflow.ts:171`
  - title: `title-workflow.ts:33`
  - chat title: `apps/desktop/src/chat/store/chat-title.ts:33`
  - Ask agent: `apps/desktop/src/chat/transport/index.ts:304,440-700`
  - classifier: `chat/guardrails/deps.ts:80`
  - pre-meeting brief: `session/insights/pre-meeting.ts:312`
  - past notes: `session/insights/past-notes.ts:586`
  - contact summary: `contacts/contact-summary.ts:264`
  - template inference: `templates/auto-format-inference.ts:51`
  - health check: `settings/ai/llm/health.tsx:34`
- **None goes through an "Efficiency Engine"**, because there isn't one. "1 call per meeting" is not a property of this code: enhancing a meeting runs the summary call plus a title call.
- **Ask tools** (`chat/tools/index.ts:79-131`): there's no delete or export tool, which is good.
  - The "This meeting" scope pins three read tools to the current session id (`chat/guardrails/tools.ts:6-10,62-100`).
  - "All meetings" can read across every meeting (`:19-31`).
  - **Gap** [V]: a chat message without an `askScope` skips the guardrails and gets the _full_ tool set, including web search and write tools (`chat/transport/index.ts:612-620`). That's the automations path, and its system prompt has no "data, not instructions" rule.
- **Untrusted content in prompts:**
  - Ask wraps sources in `<meeting_sources>`, escapes them, and says to treat them as data (`chat/guardrails/context.ts:54-128`, `prompt.ts:21`).
  - Enhance and title wrap content in tags with a data rule (`crates/template-app/assets/enhance.system.md.jinja:27-31`), but **don't escape**: the templates use `ext = "txt"` (`crates/template-app/src/types.rs:66`). So a transcript containing `</transcript>` can close the block early [V].

## 5. Logging, telemetry and error reports

- **Logs:** tracing at info level, written to stderr and `app.log` (5 MB × 5), redacted for home paths, emails, IPs, UUIDs and bearer/api_key/token values (`plugins/tracing/src/lib.rs:87-127`, `redaction.rs:1-105`).
  - Webview `console.*` lines are logged as level plus argument count only (`plugins/tracing/src/ext.rs:20-63`).
  - Users can't delete logs from the app (no command), only in Finder.
  - No transcript text found in Rust log calls. GUESS: the local STT sidecar's stdout is logged after a filter (`plugins/local-stt/src/server/external.rs:136-147`), which could leak if the sidecar's output format changes.
- **Sentry:**
  - Rust: only if a DSN was set at build time, and consent defaults to _off_ when the row is missing (`apps/desktop/src-tauri/src/lib.rs:61-83,193-237`). `traces_sample_rate: 1.0`.
  - Frontend: scrubbed `beforeSend` (`apps/desktop/src/error-reporting.ts:164-241`).
  - **Session Replay** is on for 10% of sessions and 100% on error, with masked text, whenever consent is on.
- **PostHog analytics: on by default.**
  - `telemetry_consent` defaults to true (`apps/desktop/src/settings/schema.ts:249-253`) [V].
  - The distinct id is a machine fingerprint. Events carry version, plan, provider and model, and no content in the properties checked (`plugins/analytics/src/ext.rs:33-196`).
  - The UI shows `crash_reporting_consent` default true while Rust treats a missing row as off. That mismatch is a bug.

## 6. Roles

**Single-user today when signed out**, but not by design:

- `workspaces`, `workspace_memberships.role` (no CHECK constraint), `owner_user_id`, and sharing capabilities (`viewer`/`commenter`/`editor`) are already in the local schema (`crates/db-app/migrations/20260716120000_personal_workspaces.sql`, `20260716173000_shared_session_cache.sql`).
- The hosted side (`supabase/`) has `role IN ('owner','admin','member')` with RLS policies.
- **There is no central permission check anywhere in the desktop app.** Roles are never evaluated locally.

## Section audit, A–G

| Item                                | Status before | Evidence                                                                                                                                                                                  | Gap                                                                                                                                                                                              |
| ----------------------------------- | ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A1 RBAC                             | PARTIAL       | the role/workspace/share schema above                                                                                                                                                     | no local role model, no matrix, no central check                                                                                                                                                 |
| A2 Auth on every endpoint           | MISSING       | §2                                                                                                                                                                                        | no caller/session check; ACL grants everything to every window                                                                                                                                   |
| A3 Least privilege                  | PARTIAL       | calendar is read-only with tokens in the Keychain [V]; Info.plist requests contacts, calendars, reminders and local network                                                               | over-broad capabilities, `http` scope, asset scope, `shell:allow-open` (`default.json:76`)                                                                                                       |
| A4 Session/device, app lock         | PARTIAL       | Touch ID/password lock off by default (`schema.ts:259-262`, `plugins/local-auth/src/macos.rs:41-61`)                                                                                      | lock is UI-only; data stays readable via IPC, CLI and MCP; silently skipped when device auth is unavailable (`lock/gate.tsx:39`)                                                                 |
| B1 Hardening checklist              | PARTIAL       | SQL values bound with `?`; identifier `format!` uses allowlists (`crates/db-app/src/session_deletion.rs:29-37`); no `Math.random` tokens; no `dangerouslySetInnerHTML`/`innerHTML` writes | raw-SQL command; no CSP; no `cargo audit`/`deny`/`pnpm audit` in CI; no constant-time compares                                                                                                   |
| B2 Input validation                 | PARTIAL       | import formats are JSON/VTT only (`crates/meeting-import`)                                                                                                                                | unchecked import paths; `render_custom`; webhook URLs; no response schema checks on calendar/AI                                                                                                  |
| B3 Prompt injection                 | PARTIAL       | Ask guardrails, no delete/export tools                                                                                                                                                    | unscoped automations path; unescaped enhance/title blocks; no injection tests                                                                                                                    |
| B4 Cross-meeting isolation in Ask   | PARTIAL       | "This meeting" tool pinning (`chat/guardrails/tools.ts:62-100`)                                                                                                                           | no explicit test; Tantivy search has no session filter (`plugins/tantivy/src/lib.rs:70-74`)                                                                                                      |
| B5 Update signatures                | EXISTS (gap)  | minisign pubkey `tauri.conf.json:115`; verified on download                                                                                                                               | cached installer isn't re-verified before install (`plugins/updater2/src/ext.rs:245-258`)                                                                                                        |
| C1 Tenant isolation                 | PARTIAL       | columns exist and some ops filter by `workspace_id` (`crates/db-app/src/session_deletion.rs:37-75`)                                                                                       | default `''`; most frontend SQL doesn't filter; no 2-workspace test                                                                                                                              |
| C2 Encryption at rest               | MISSING       | no SQLCipher; files are plaintext                                                                                                                                                         | relies on FileVault, unchecked. Network: the `http` scope allows `http://**` (not TLS-only)                                                                                                      |
| C3 Key protection                   | PARTIAL       | §3                                                                                                                                                                                        | webview reads keys; `auth.json`; webhook secrets; keys in URL query strings                                                                                                                      |
| C4 Log/telemetry hygiene            | PARTIAL       | redaction layer and Sentry scrubbing                                                                                                                                                      | no marker-string test; Session Replay; sidecar stdout                                                                                                                                            |
| C5 Backup/restore                   | MISSING       | no full-library backup; `.bak` on reset is incidental (`apps/desktop/src-tauri/src/db.rs:53-76`)                                                                                          | everything                                                                                                                                                                                       |
| C6 Recording protection             | PARTIAL       | tray animation, floating bar, Info.plist strings, onboarding text, consent-disclosure option                                                                                              | auto-start defaults **on** (`schema.ts:73-77`) [V] and both indicators can be turned off, so recording without an in-app indicator is possible; no "Private" flag (`sessions.locked` is closest) |
| C7 Cost protection                  | MISSING       | optional Ask-only daily limit (`apps/desktop/src/ai/usage.ts:220`)                                                                                                                        | no Engine, model lock, quotas, spend caps or clean fallback state                                                                                                                                |
| D1 Immutable audit log              | MISSING       | the sync "log" is unrelated (`settings/sync/log.tsx`)                                                                                                                                     | everything                                                                                                                                                                                       |
| D2 Data rights                      | PARTIAL       | per-note export (PDF/TXT/MD/ORG); CLI hard delete with VACUUM (`apps/cli/src/commands/meeting_delete.rs:118-262`)                                                                         | app delete is a soft delete that keeps rows forever (`plugins/session/src/deletion.rs:53-94`); versions, live deltas, chats and the search index survive; no export-all; no delete-library       |
| D3 Retention                        | PARTIAL       | audio-only retention sweep (`plugins/transcription/src/audio_retention.rs:117-313`)                                                                                                       | nothing for notes, transcripts or rows                                                                                                                                                           |
| D4 Privacy surfaces                 | MISSING       | `docs/data-and-privacy.mdx` exists outside the app                                                                                                                                        | no in-app policy/terms links or "where your data lives" screen; no consent notice before the first recording                                                                                     |
| D5 Licence                          | PARTIAL       | MIT `LICENSE` kept; no imports from `enterprise/`; CI boundary check (`.github/workflows/ci.yaml:12-14`)                                                                                  | no About/Licences screen                                                                                                                                                                         |
| D6 No content in analytics/training | PARTIAL       | event properties carry no content                                                                                                                                                         | not stated as a rule; Session Replay records the UI                                                                                                                                              |
| E Attack hardening                  | see below     |                                                                                                                                                                                           |                                                                                                                                                                                                  |
| F Pen-test gate                     | MISSING       |                                                                                                                                                                                           | no process                                                                                                                                                                                       |
| G Pre-ship ops                      | PARTIAL       | CI, signing and release audit exist (`RELEASE_AUDIT.md`)                                                                                                                                  | ASPC items not in it                                                                                                                                                                             |

### E. Attack hardening

| Attack                                   | Verdict                  | Evidence                                                                                                                                                                                                                                                                                                           |
| ---------------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| SQL injection                            | **APPLIES (reduced)**    | values are bound, but the webview's raw-SQL command is equivalent to injection by design (`plugins/db/src/commands.rs:110-150`). Since Phase 1 it runs under an SQLite authorizer that blocks ATTACH, extensions, pragmas, DDL and audit tables (Phase 1 → A2.4); reads and writes of app tables are still allowed |
| HTTP/2 Rapid Reset                       | N/A                      | the desktop app serves no HTTP/2. GUESS: the hosted `apps/api` is behind its platform's edge (not in scope)                                                                                                                                                                                                        |
| Forged SAML                              | N/A locally              | there's no SAML in the desktop app. The hosted SSO is Supabase's (`supabase/migrations/20260826120000_enforce_required_sso.sql`)                                                                                                                                                                                   |
| Username truncation                      | N/A                      | there are no local usernames or passwords                                                                                                                                                                                                                                                                          |
| GraphQL-alias 2FA brute force            | N/A                      | there's no GraphQL                                                                                                                                                                                                                                                                                                 |
| 2FA/OTP replay                           | N/A                      | there's no OTP. The OAuth `state` is single-use (`crates/local-accounts/src/loopback.rs:152-173`)                                                                                                                                                                                                                  |
| ReDoS                                    | low                      | guardrail regexes run on normalised single-spaced input (`chat/guardrails/classifier.ts:50-122`). Transcript regexes are unchecked. Needs a fuzz test                                                                                                                                                              |
| JWT algorithm confusion                  | **APPLIES (low)**        | `Claims::decode_insecure` drives UI plan gating (`plugins/auth/src/commands.rs:5-6`). The server must stay the authority                                                                                                                                                                                           |
| HTTP request smuggling                   | N/A                      | no proxy in the desktop app                                                                                                                                                                                                                                                                                        |
| Web cache poisoning                      | N/A                      | no shared cache; the local servers don't cache                                                                                                                                                                                                                                                                     |
| Spoofed IP headers bypassing rate limits | N/A                      | no IP-based limits locally                                                                                                                                                                                                                                                                                         |
| Reading past row/workspace security      | **APPLIES**              | no row-level enforcement; raw SQL reads anything                                                                                                                                                                                                                                                                   |
| Insecure `Math.random()` tokens          | OK                       | only UI and fallback-id uses (`apps/desktop/src/main/home.tsx:149`, `stt/meeting-disclosure.ts:123`)                                                                                                                                                                                                               |
| Timing attacks                           | **APPLIES (low, local)** | plain `!=` on the OAuth state (`crates/local-accounts/src/loopback.rs:173`) and the relay cookie (`plugins/relay/src/access.rs:36-43`). Hosted: `crates/nango/src/webhook.rs:49-55`, `crates/transcribe-proxy/src/routes/callback.rs:31`                                                                           |
| Padding oracle                           | N/A                      | AEAD only (XChaCha20-Poly1305)                                                                                                                                                                                                                                                                                     |
| Prototype pollution                      | N/A found                | no deep-merge of external JSON. Keep it that way                                                                                                                                                                                                                                                                   |
| Template injection (SSTI)                | **APPLIES (low)**        | `render_custom` and the user prompt override are MiniJinja (sandboxed, no fuel limit) (`crates/template-app/src/enhance.rs:22-38`)                                                                                                                                                                                 |
| XXE                                      | N/A                      | no XML parsing of imported files                                                                                                                                                                                                                                                                                   |
| WebSocket hijacking (CSWSH)              | **APPLIES (low)**        | the local STT WebSocket has no Origin check (`plugins/local-stt/src/server/internal.rs:23-33`)                                                                                                                                                                                                                     |
| Unicode impersonation                    | **APPLIES (low)**        | NFKD/NFKC matching exists but there's no confusable detection for participant names or emails                                                                                                                                                                                                                      |
| argon2/bcrypt                            | N/A                      | no passwords are stored                                                                                                                                                                                                                                                                                            |
| Security headers / CSP                   | **APPLIES**              | no CSP; `withGlobalTauri: true` [V]                                                                                                                                                                                                                                                                                |
| CORS allowlist                           | **APPLIES**              | local STT uses `Any`                                                                                                                                                                                                                                                                                               |
| CSRF                                     | **APPLIES (low)**        | the deeplink2 callback has no Rust state check; the ChatGPT flow allows a missing state                                                                                                                                                                                                                            |
| XSS output encoding                      | OK today                 | React escaping, no raw HTML sinks. CSP missing as a second layer                                                                                                                                                                                                                                                   |
| Rate limiting on IPC                     | MISSING                  | none                                                                                                                                                                                                                                                                                                               |
| Rotate leaked keys                       | none found leaked        | see §3 scan                                                                                                                                                                                                                                                                                                        |

## Phase 1 changes (webview lockdown), 2026-11-16

Status of each Phase 1 item after the change. File references are to the code as committed.

### A2.4 SQL authorizer: PARTIAL

- **What exists now:**
  - `crates/db-core/src/webview_sql.rs`: an `sqlite3_set_authorizer` callback (`authorizer_callback`, :175) and a pure policy function (`decide`, :103).
  - The authorizer is installed on a pool connection only for the duration of a webview statement (`AuthorizedConnection`, :204) and cleared before the connection returns to the pool. If the statement is cancelled, a task clears it after the in-flight statement finishes; if that fails, the connection is closed instead.
  - Every webview SQL path uses it:
    - `db:execute` / `execute_proxy` through `DbExecutor::for_webview` (`crates/db-execute/src/lib.rs:64`, wired at `plugins/db/src/runtime.rs:182`);
    - `db:execute_transaction` (`plugins/db/src/runtime.rs:517-596`);
    - live queries (`db:subscribe`): initial load, refreshes and the `EXPLAIN QUERY PLAN {sql}` dependency analysis (`crates/db-reactive/src/runtime.rs:36`, `crates/db-reactive/src/schema.rs:255`). The EXPLAIN path mattered: a subscription ending in `; ATTACH ...` used to run the second statement (regression test `webview_runtime_never_runs_smuggled_statements`).
  - Internal Rust paths (migrations, CloudSync, importers, `agent-access`) keep using the pool without the authorizer.
- **Policy:** denies `ATTACH`/`DETACH`; the `load_extension` and `fts3_tokenizer` functions; every `PRAGMA` except a read-only allowlist (`webview_sql.rs:24-45`; the frontend itself uses none, the allowlist covers what the CloudSync extension runs internally during writes); any write to `sqlite_*` tables; all DDL (CREATE/DROP/ALTER of tables, indexes, views, triggers, virtual tables), `REINDEX` and `ANALYZE`; any read or write of a table whose name starts with `audit`; every action code it doesn't know (fail closed).
- **Lock flag:** `WebviewSqlAuthorizer::set_locked` (`webview_sql.rs:62`), exposed as `PluginDbRuntime::set_webview_sql_locked` (`plugins/db/src/runtime.rs:599`). Default unlocked. While locked every webview statement is denied; internal paths are not. **Not wired to the app lock yet** (Phase 2).
- **Tests:** `crates/db-core/src/webview_sql.rs` (denials, normal SELECT/INSERT/UPDATE/DELETE, lock, internal queries unaffected after release, unknown action denied, CloudSync-enabled table accepts webview writes and records them), `crates/db-execute/src/query.rs` (`webview_executor_runs_under_authorizer`), `plugins/db/src/tests/transactions.rs` (`webview_sql_runs_under_the_authorizer_and_lock`, plus the existing cancellation test still passes).
- **Still missing (why PARTIAL):** the per-table `DataType` / Read-vs-Edit mapping and the `Viewer` write denial from A2.4's first bullet (they need Phase 2's `authorize()`); the frontend can still read and write every app table, including `webhook_endpoints.secret`. The CLI/MCP path (A2.7) doesn't use the authorizer.

### Hooks feature: REMOVED

- Deleted `plugins/hooks` (`tauri-plugin-hooks`, JS package `@anlg/plugin-hooks`) and `crates/hooks` (`anlg-hooks`), their workspace entries in `Cargo.toml`, the app dependency in `apps/desktop/src-tauri/Cargo.toml`, the registration in `apps/desktop/src-tauri/src/lib.rs`, the `hooks:default` capability, the `--exclude tauri-plugin-hooks` line in `.github/workflows/desktop_ci.yaml`, the `@anlg/plugin-hooks` dependency in `apps/desktop/package.json` and `pnpm-lock.yaml`.
- Removed both `runEventHooks` callers (`beforeListeningStarted`, `afterListeningStopped`) from `apps/desktop/src/store/zustand/listener/general-live.ts`; starting a recording no longer needs `settings:vault_base`.
- There was no settings-schema entry or settings UI for `hooks` (the plugin read the raw `hooks` key from `settings.json`). A leftover key is ignored.
- Removed the "shell hooks" claim from `apps/web/content/articles/char-vs-meetily.mdx`.

## Not verified

- No runtime test was run for any of this; it's a read-only audit.
- The two GUESSes on the most important path: the plain `-wal`/`.bak` copies of old keys, and whether hidden-window auto-start shows any indicator.

## Proposed design for A1, A2 and C1

Awaiting approval. Nothing below has been built.

### A1: central permission check

- **New crate `crates/aspc`**, dependency-free, so the hosted API can use the same crate later. It defines:
  - `Role`, `DataType`, `Action`, and the matrix from ASPC_BASE §A1;
  - `AccessContext`;
  - `authorize()`, a pure function with table-driven tests;
  - generated TS types.
- **Local mode:**
  - At startup, Rust builds the `AccessContext`: actor = a stable local actor id stored in the Keychain; role = `Owner`; workspace = the device's personal workspace.
  - Reuse the existing `workspaces` row, or create one with `kind='personal'`.
  - The context lives in Tauri state.
- **Signed-in team mode:** role comes from `workspace_memberships.role`. That column has no CHECK constraint today; unknown values map to `Viewer`.

### A2: one gate in front of every command

Four layers, all in Rust:

1. **Window capabilities.** Split `default.json` into:
   - `main` (data access);
   - `floating-bar`, `dictation-overlay`, `live-caption` and `composer` (only their own commands).
     Note windows get the same rights as `main`.
2. **Command registry.** A table mapping each command to the `(DataType, Action)` it needs, or `Public`, with a reason.
   - A test reads every plugin's generated permission manifest and fails if a command is missing from the table.
   - Commands call `aspc::require(&app, &window, data, action)?` as their first line.
   - That is about 120 data commands. The other ~330 are `Public` (window, permission, audio-device and UI state).
3. **SQL authorizer on the webview connection.** `db:execute*` uses a connection with `sqlite3_set_authorizer`:
   - read and write per table, mapped to `DataType`;
   - deny `ATTACH`, `load_extension`, `audit*` and `PRAGMA`s off the allowlist;
   - deny all when locked;
   - deny writes for `Viewer`.

   This keeps the existing frontend (Drizzle over raw SQL) working without a rewrite. Through `libsqlite3-sys`, which is already a dependency (`crates/db-core/Cargo.toml:14`).
4. **Remove or restrict the dangerous commands:**
   - `get_env`: allowlist only.
   - `fs2:write_text_file` and `export`: write only to the vault, or to a path just picked in a dialog.
   - `hooks`: needs re-auth to change, and shows the exact command.
   - `store2:get_secret` for provider keys: denied once the Efficiency Engine makes AI calls in Rust.
   - `opener2`: no `open_with`.
   - `http` scope: HTTPS allowlist.
   - Asset scope: the vault only.
   - Add a CSP. Set `withGlobalTauri: false`; nothing in the frontend uses it.

   The **CLI/MCP** gets the same authorizer and lock check.

### C1: workspace isolation

- **Backfill:** every `''` `workspace_id` and `owner_user_id` gets the local personal workspace and actor ids, in one migration that changes data, not schema.
  - New rows get them from the `AccessContext`, never from a default.
- **Reads:** Rust query functions (`agent-access`, export, Ask tools, Tantivy search) take a `WorkspaceScope` argument and filter by it.
  - Raw webview SQL is limited to the active workspace. The authorizer can't filter rows, so the webview connection gets `TEMP` views per content table, with base-table reads denied.
  - **This is the riskiest part**, because Drizzle and the reactive layer name base tables. **Fallback:** the authorizer allows base tables only in local single-workspace mode, and the views come in with the hosted version. The 2-workspace test then runs against the Rust paths. I'd decide between these after a spike.
- **Search:** Tantivy documents get `workspace_id` and `session_id` fields and filters (also needed for B4).

## Decisions needed

1. **Efficiency Engine (C7, verification item 10).** It doesn't exist. My proposal:
   - a Rust gateway in `plugins/ai-engine` with Haiku locked;
   - a usage counter, quotas, rate limits, the fallback state, and one call per enhance;
   - the webview calls only the gateway.

   **Open question:** does the "developer proxy key" mean
   - (a) a key held on this Mac, or
   - (b) a hosted proxy with spend caps?

   (b) is a network backend, which needs your approval. Also: should the Engine replace the user's own provider and model choice, or sit beside it?
2. **Encryption at rest (C2).** The choice is between:
   - **(a) SQLCipher.** Use `libsqlite3-sys` with `bundled-sqlcipher` and a Keychain-held key, and encrypt audio with XChaCha20 streams.
     - Risks: the CloudSync extension and the CLI must load against SQLCipher; mobile shares the schema.
     - Encrypted audio breaks asset-protocol playback, so it needs a decrypting stream.
     - About a week of work.
   - **(b) FileVault.** Check FileVault at startup, set `0700`/`0600` permissions, and keep the Keychain for secrets. Document the limits: no protection while the Mac is unlocked and logged in, or from other processes running as the same user.
     - Migration path: (b) now, (a) when hosted sync ships.

   I recommend (b) now and (a) later, but this is your call.
3. **Schema changes.** Each needs your approval:
   - a new `audit.db` file and table (D1);
   - a `sessions.private` column (C6.4);
   - Tantivy schema fields `workspace_id` and `session_id`;
   - a data backfill of empty workspace and owner ids (C1).

   Alternative for Private: reuse the existing `locked` flag to mean Private, with no new column.
4. **Defaults that change behaviour:**
   - `telemetry_consent` and Session Replay are on by default today. I propose off (D6, C4.3).
   - `auto_start_scheduled_meetings` is on by default today, and indicators can be hidden. I propose forcing an indicator during recording (C6.1), and leaving auto-start on but with a notification.
5. **Soft delete (D2.2).** Deleting a meeting would become a permanent delete after the 5-second undo, instead of keeping tombstoned rows forever. Sync relies on tombstones; for synced meetings I'd keep a tombstone row with the content columns blanked.
6. ~~**Hooks feature.**~~ **Decided 2026-11-16: removed.** See ASPC_BASE "Decisions".
