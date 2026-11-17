# ASPC audit: Access, Security, Protection, Compliance

Audit of the `prototype` branch of this fork, done on 2026-11-16, before any ASPC code changes. Phase 1 (webview lockdown, same day) changed some items; their current status is in "Phase 1 changes" below, and the tables mark them.
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

1. **There is no "Efficiency Engine".** No code or docs by that name exist, and there's no locked Haiku model. _(Built 2026-11-16: see "Phase 3 changes".)_
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
| `fs2:write_text_file` (`plugins/fs2/src/commands.rs:27-32`) [V]                                 | **PARTIAL (Phase 1):** vault/app data or a dialog-picked path only, see Phase 1 → A3.6.                                                                                                                                                          |
| `export:export` (`plugins/export/src/commands.rs:7-15`)                                         | **PARTIAL (Phase 1):** same check as `fs2:write_text_file`.                                                                                                                                                                                      |
| ~~`settings:save` + `hooks:run_event_hooks` (`crates/hooks/src/runner.rs:48-65`)~~              | **FIXED (Phase 1):** the hooks feature was removed; see Phase 1 → Hooks.                                                                                                                                                                         |
| `opener2:open_path` / `open_url` (`plugins/opener2/src/ext.rs:10-18`)                           | **PARTIAL (Phase 1):** `open_with` is rejected unless on an (empty) allowlist; see Phase 1 → opener2.                                                                                                                                            |
| `get_env(key)` (`apps/desktop/src-tauri/src/commands.rs:45-47`) [V]                             | **FIXED (Phase 1):** allowlist, see Phase 1 → A3.7.                                                                                                                                                                                              |
| `store2:get_secret`, `auth:get_item`                                                            | Return LLM/STT keys and sign-in tokens to JS.                                                                                                                                                                                                    |
| `local-api:create_webhook` (`plugins/local-api/src/dispatch.rs:123-151`)                        | **PARTIAL (Phase 1):** https only, public addresses only, checked again on delivery; see Phase 1 → B2.2.                                                                                                                                         |
| `http:default` scope (`capabilities/default.json:99-116`) [V]                                   | **PARTIAL (Phase 1):** host allowlist plus user-confirmed origins; see Phase 1 → A3.3.                                                                                                                                                           |
| asset protocol scope `**/*` (`apps/desktop/src-tauri/tauri.conf.json:18-23`) [V]                | **EXISTS (Phase 1):** app cache plus the vault's `sessions/`; see Phase 1 → A3.2.                                                                                                                                                                |
| `importer:read_text_files` (`plugins/importer/src/commands.rs:69-94`)                           | **PARTIAL (Phase 1):** vault or dialog-picked paths only.                                                                                                                                                                                        |
| `fs-sync:audio_import` (`plugins/fs-sync/src/commands.rs:314-326`)                              | **PARTIAL (Phase 1):** vault or dialog-picked source only.                                                                                                                                                                                       |
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

| Secret                                        | Where today                                                                                                                                 | Target                                           | Status                                                                                                                                                                   |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Anthropic, OpenAI and other LLM/STT BYOK keys | Keychain `secure-store`, scope `ai-provider-api-keys` (`apps/desktop/src/settings/providers.ts:24,141-145`)                                 | Keychain, not readable by the webview            | PARTIAL (Phase 3): LLM keys (`llm:*`) are refused to the webview (`plugins/store2/src/commands.rs:31,47,363`) and no longer used; STT keys are still read by the webview |
| Legacy plaintext keys in `app_settings`       | migrated out at startup with `secure_delete` (`providers.ts:467-561`)                                                                       | gone                                             | PARTIAL: still used as a fallback if the Keychain is empty (`providers.ts:64`); GUESS: old WAL/`.bak` pages may keep them                                                |
| Claude/ChatGPT subscription OAuth             | was the provider `api_key`; the subscription sign-in code is removed (Phase 3)                                                              | Keychain                                         | N/A: removed; old items stay in the Keychain under `llm:*`, unreadable by the webview                                                                                    |
| Google/Microsoft calendar refresh tokens      | Keychain `<id>.accounts`, native only (`crates/local-accounts/src/store.rs`) [V]                                                            | Keychain                                         | EXISTS                                                                                                                                                                   |
| Google OAuth "client secret"                  | env, or `option_env!` into the binary (`crates/local-accounts/src/provider.rs:64-88`) [V]                                                   | public by Google's design for desktop clients    | EXISTS (documented as non-confidential)                                                                                                                                  |
| Supabase session                              | `auth.json` plaintext on macOS (`plugins/auth/src/migrate.rs:8`)                                                                            | Keychain                                         | MISSING (only affects hosted sign-in)                                                                                                                                    |
| Webhook secrets                               | `webhook_endpoints.secret` plaintext in SQLite                                                                                              | Keychain                                         | MISSING                                                                                                                                                                  |
| E2EE recovery and device keys, voiceprint     | Keychain, native only (`plugins/db/src/commands.rs:22-29`)                                                                                  | Keychain                                         | EXISTS                                                                                                                                                                   |
| Efficiency Engine key (DIRECT mode)           | Keychain service `<bundle id>.ai-engine`, item `anthropic-api-key` (`crates/ai-engine/src/config.rs:23-24`, `plugins/ai-engine/src/lib.rs`) | Keychain; server-side in PROXY mode **[hosted]** | EXISTS: written by `set_api_key`, read only by Rust, never returned (`key_status` gives presence only)                                                                   |

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
- **Phase 3 status (2026-11-16):** every call site above now gets the Engine model from `useLanguageModel` (`apps/desktop/src/ai/hooks/useLLMConnection.ts:49`) → `createEngineModel` (`apps/desktop/src/ai/engine/model.ts:25`) → `createEngineFetch` (`transport.ts:68`) → `ai-engine:request` (`plugins/ai-engine/src/commands.rs:29`). The health check (`settings/ai/llm/health.tsx`) is removed with the picker. Enhance and title are one call (`enhance-workflow.ts:27-28,183`, `enhance-success.ts:71`). The unscoped chat gap is closed (`chat/transport/index.ts:76,626`). Enhance and title content is escaped (`crates/template-app/src/escape.rs:32`, `lib.rs:81,85`).
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
- ~~**There is no central permission check anywhere in the desktop app.**~~ Phase 2a added one: `crates/aspc` (`authorize`, `crates/aspc/src/lib.rs:250`). Locally the actor is the `Owner` of the personal workspace; `workspace_memberships.role` is not read yet (signed-in team mode is Phase 2b).

## Section audit, A–G

| Item                                | Status before      | Evidence                                                                                                                                                                                                                                                                 | Gap                                                                                                                                                                                                      |
| ----------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1 RBAC                             | PARTIAL (Phase 2a) | `crates/aspc` roles, matrix, `authorize` (`crates/aspc/src/lib.rs:237-336`), table-driven tests incl. A1.4; role-comparison test with allowlist (`crates/aspc/tests/role_comparisons.rs`)                                                                                | no TS mirror generated; team roles from `workspace_memberships` not read (Phase 2b); `Member` owner checks only where a caller passes the owner (none yet)                                               |
| A2 Auth on every endpoint           | PARTIAL (Phase 2a) | registry of all 460 commands (`crates/aspc/src/commands.rs:113`) with a sync test; guard `aspc::require` (`crates/aspc/src/tauri_guard.rs:46`) in 34 commands, lock-exempt guard in 8; per-table SQL authorizer (`crates/db-core/src/webview_sql.rs:65`, `:122`)         | 234 data commands `Pending` (listed under "Phase 2a changes"); SQL authorizer has no window and no row/workspace filter; CLI/MCP don't use it (A2.7); no rate limits (A2.8)                              |
| A3 Least privilege                  | PARTIAL (Phase 1)  | calendar is read-only with tokens in the Keychain [V]; CSP, per-window capabilities, HTTP host allowlist, narrowed asset scope, path checks, `get_env` allowlist (Phase 1)                                                                                               | Info.plist still requests contacts, calendars, reminders and local network; `shell:allow-open` stays in `main.json`; see each Phase 1 item's gaps                                                        |
| A4 Session/device, app lock         | PARTIAL (Phase 2a) | Rust-held lock (`crates/aspc/src/lock.rs`, `plugins/local-auth/src/app_lock.rs`); unlock only via Rust-run LocalAuthentication (`plugins/local-auth/src/app_lock.rs:84`); webview SQL locked from it (`apps/desktop/src-tauri/src/lib.rs:788`); relock on hide and sleep | no idle timeout (no setting); `Pending` and lock-exempt commands still answer while locked; CLI/MCP ignore the lock; file modes (A4.1) unchecked; not run in the app                                     |
| B1 Hardening checklist              | PARTIAL            | SQL values bound with `?`; identifier `format!` uses allowlists (`crates/db-app/src/session_deletion.rs:29-37`); no `Math.random` tokens; no `dangerouslySetInnerHTML`/`innerHTML` writes                                                                                | raw-SQL command; no CSP; no `cargo audit`/`deny`/`pnpm audit` in CI; no constant-time compares                                                                                                           |
| B2 Input validation                 | PARTIAL            | import formats are JSON/VTT only (`crates/meeting-import`)                                                                                                                                                                                                               | unchecked import paths; `render_custom`; webhook URLs; no response schema checks on calendar/AI                                                                                                          |
| B3 Prompt injection                 | EXISTS (Phase 3)   | Ask guardrails, no delete/export tools; enhance/title escaping (`crates/template-app/src/escape.rs:32`); unscoped requests get read tools only (`chat/transport/index.ts:626`); injection tests (`enhance-one-call.test.ts`, `chat/guardrails/injection.test.ts`)        | `render_custom` callers (pre-meeting brief, past notes) are escaped per value but their templates were not reviewed for a data rule; chat context block (`context.block.md.jinja`) not escaped           |
| B4 Cross-meeting isolation in Ask   | EXISTS (Phase 3)   | "This meeting" tool pinning (`chat/guardrails/tools.ts:62-100`); Tantivy `session_id` filter and B4.2 search-layer test (Phase 2b); Ask-level stub-model test (`chat/guardrails/injection.test.ts`)                                                                      | not run in the app                                                                                                                                                                                       |
| B5 Update signatures                | EXISTS (gap)       | minisign pubkey `tauri.conf.json:115`; verified on download                                                                                                                                                                                                              | cached installer isn't re-verified before install (`plugins/updater2/src/ext.rs:245-258`)                                                                                                                |
| C1 Tenant isolation                 | PARTIAL            | columns exist and some ops filter by `workspace_id` (`crates/db-app/src/session_deletion.rs:37-75`)                                                                                                                                                                      | webview SQL is guarded, not filtered (interim); Rust write helpers unscoped                                                                                                                              |
| C2 Encryption at rest               | MISSING            | no SQLCipher; files are plaintext                                                                                                                                                                                                                                        | relies on FileVault, unchecked. Network: the `http` scope allows `http://**` (not TLS-only)                                                                                                              |
| C3 Key protection                   | PARTIAL            | §3; LLM keys refused to the webview and the Engine key read only in Rust (Phase 3)                                                                                                                                                                                       | webview still reads STT and import keys; `auth.json`; webhook secrets; STT keys in URL query strings                                                                                                     |
| C4 Log/telemetry hygiene            | PARTIAL            | redaction layer and Sentry scrubbing                                                                                                                                                                                                                                     | no marker-string test; Session Replay; sidecar stdout                                                                                                                                                    |
| C5 Backup/restore                   | MISSING            | no full-library backup; `.bak` on reset is incidental (`apps/desktop/src-tauri/src/db.rs:53-76`)                                                                                                                                                                         | everything                                                                                                                                                                                               |
| C6 Recording protection             | PARTIAL            | tray animation, floating bar, Info.plist strings, onboarding text, consent-disclosure option                                                                                                                                                                             | auto-start defaults **on** (`schema.ts:73-77`) [V] and both indicators can be turned off, so recording without an in-app indicator is possible; Private flag built in Phase 4 (C6.4, `sessions.private`) |
| C7 Cost protection                  | EXISTS (Phase 3)   | `crates/ai-engine`, `plugins/ai-engine`; see "Phase 3 changes"                                                                                                                                                                                                           | PROXY mode is a stub; spend caps **[hosted]**; not run in the app                                                                                                                                        |
| D1 Immutable audit log              | EXISTS (Phase 4)   | separate `audit.db`, append-only triggers, hash chain with the head in the Keychain, Settings → Audit with Verify (`crates/audit-log`, `plugins/audit`); see "Phase 4 changes"                                                                                           | everything                                                                                                                                                                                               |
| D2 Data rights                      | PARTIAL            | per-note export (PDF/TXT/MD/ORG); permanent delete after the undo window (Phase 4, D2.2); CLI hard delete with VACUUM (`apps/cli/src/commands/meeting_delete.rs:118-262`)                                                                                                | app delete is a soft delete that keeps rows forever (`plugins/session/src/deletion.rs:53-94`); versions, live deltas, chats and the search index survive; no export-all; no delete-library               |
| D3 Retention                        | EXISTS (Phase 4)   | "Delete meetings older than N days" job through the permanent delete (`plugins/session/src/retention.rs`); audio-only sweep unchanged (`plugins/transcription/src/audio_retention.rs:117-313`)                                                                           | nothing for notes, transcripts or rows                                                                                                                                                                   |
| D4 Privacy surfaces                 | MISSING            | `docs/data-and-privacy.mdx` exists outside the app                                                                                                                                                                                                                       | no in-app policy/terms links or "where your data lives" screen; no consent notice before the first recording                                                                                             |
| D5 Licence                          | PARTIAL            | MIT `LICENSE` kept; no imports from `enterprise/`; CI boundary check (`.github/workflows/ci.yaml:12-14`)                                                                                                                                                                 | no About/Licences screen                                                                                                                                                                                 |
| D6 No content in analytics/training | PARTIAL            | event properties carry no content                                                                                                                                                                                                                                        | not stated as a rule; Session Replay records the UI                                                                                                                                                      |
| E Attack hardening                  | see below          |                                                                                                                                                                                                                                                                          |                                                                                                                                                                                                          |
| F Pen-test gate                     | MISSING            |                                                                                                                                                                                                                                                                          | no process                                                                                                                                                                                               |
| G Pre-ship ops                      | PARTIAL            | CI, signing and release audit exist (`RELEASE_AUDIT.md`)                                                                                                                                                                                                                 | ASPC items not in it                                                                                                                                                                                     |

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
| Security headers / CSP                   | **PARTIAL (Phase 1)**    | CSP added and `withGlobalTauri: false`; see Phase 1 → A3.1 for the remaining allowances                                                                                                                                                                                                                            |
| CORS allowlist                           | **APPLIES**              | local STT uses `Any`                                                                                                                                                                                                                                                                                               |
| CSRF                                     | **APPLIES (low)**        | the deeplink2 callback has no Rust state check; the ChatGPT flow allows a missing state                                                                                                                                                                                                                            |
| XSS output encoding                      | OK today                 | React escaping, no raw HTML sinks. CSP missing as a second layer                                                                                                                                                                                                                                                   |
| Rate limiting on IPC                     | MISSING                  | none                                                                                                                                                                                                                                                                                                               |
| Rotate leaked keys                       | none found leaked        | see §3 scan                                                                                                                                                                                                                                                                                                        |

## Phase 1 changes (webview lockdown), 2026-11-16

Status of each Phase 1 item after the change. File references are to the code as committed.

### A2.4 SQL authorizer: PARTIAL (Phase 1; EXISTS after Phase 2a, see below)

- **What exists now:**
  - `crates/db-core/src/webview_sql.rs`: an `sqlite3_set_authorizer` callback (`authorizer_callback`, :175) and a pure policy function (`decide`, :103).
  - The authorizer is installed on a pool connection only for the duration of a webview statement (`AuthorizedConnection`, :204) and cleared before the connection returns to the pool. If the statement is cancelled, a task clears it after the in-flight statement finishes; if that fails, the connection is closed instead.
  - Every webview SQL path uses it:
    - `db:execute` / `execute_proxy` through `DbExecutor::for_webview` (`crates/db-execute/src/lib.rs:64`, wired at `plugins/db/src/runtime.rs:182`);
    - `db:execute_transaction` (`plugins/db/src/runtime.rs:517-596`);
    - live queries (`db:subscribe`): initial load, refreshes and the `EXPLAIN QUERY PLAN {sql}` dependency analysis (`crates/db-reactive/src/runtime.rs:36`, `crates/db-reactive/src/schema.rs:255`). The EXPLAIN path mattered: a subscription ending in `; ATTACH ...` used to run the second statement (regression test `webview_runtime_never_runs_smuggled_statements`).
  - Internal Rust paths (migrations, CloudSync, importers, `agent-access`) keep using the pool without the authorizer.
- **Policy:** denies `ATTACH`/`DETACH`; the `load_extension` and `fts3_tokenizer` functions; every `PRAGMA` except a read-only allowlist (`webview_sql.rs:24-45`; the frontend itself uses none, the allowlist covers what the CloudSync extension runs internally during writes); any write to `sqlite_*` tables; all DDL (CREATE/DROP/ALTER of tables, indexes, views, triggers, virtual tables), `REINDEX` and `ANALYZE`; any read or write of a table whose name starts with `audit`; every action code it doesn't know (fail closed).
- **Lock flag:** `WebviewSqlAuthorizer::set_locked` (`webview_sql.rs:62`), exposed as `PluginDbRuntime::set_webview_sql_locked` (`plugins/db/src/runtime.rs:599`). Default unlocked. While locked every webview statement is denied; internal paths are not. Wired to the Rust-held app lock in Phase 2a.
- **Tests:** `crates/db-core/src/webview_sql.rs` (denials, normal SELECT/INSERT/UPDATE/DELETE, lock, internal queries unaffected after release, unknown action denied, CloudSync-enabled table accepts webview writes and records them), `crates/db-execute/src/query.rs` (`webview_executor_runs_under_authorizer`), `plugins/db/src/tests/transactions.rs` (`webview_sql_runs_under_the_authorizer_and_lock`, plus the existing cancellation test still passes).
- **Still missing (why PARTIAL):** the per-table `DataType` / Read-vs-Edit mapping and the `Viewer` write denial from A2.4's first bullet (they need Phase 2's `authorize()`); the frontend can still read and write every app table, including `webhook_endpoints.secret`. The CLI/MCP path (A2.7) doesn't use the authorizer.

### A3.1 CSP and global Tauri: PARTIAL

- `apps/desktop/src-tauri/tauri.conf.json:15` sets `withGlobalTauri: false`. Nothing in the frontend read `window.__TAURI__`. The one user was the tracing plugin's console-forwarding init script, now switched to `window.__TAURI_INTERNALS__` (`plugins/tracing/src/ext.rs:253`). Without that change it would have polled every 10 ms forever and stopped forwarding console lines.
- `app.security.csp` (`tauri.conf.json:18-31`) and `devCsp` (`:32-45`). No `tauri.conf.*.json` / `tauri.macos.conf.json` override sets `app.security`. The only override with an `app` key (`tauri.conf.app-store.json:4`) sets `macOSPrivateApi` and is merged, so every variant keeps the CSP.
  - `script-src 'self'`: no inline scripts ship (`index.html` loads `theme-boot.js` and the bundle as files; `dictation.html` has none). No `eval` / `new Function` in our code or the production bundle (grep of `vite build` output). Zod 4 probes `new Function` at startup, so `apps/desktop/src/shared/zod-jitless.ts` (imported second in `main.tsx`) sets `z.config({ jitless: true })`. No `wasm-unsafe-eval`: the only WebAssembly in the bundle is Shiki's optional Oniguruma engine, and the code-highlighting path defaults to the JS regex engine (`preferredHighlighter: "shiki-js"`). GUESS: nothing selects `shiki-wasm`.
  - `style-src 'self' 'unsafe-inline'` with `dangerousDisableAssetCspModification: ["style-src"]` (`:46`). Needed because React `style` attributes, motion animations, the editor and the inline `<style>` blocks in `index.html` / `dictation.html` use inline styles. If Tauri added its style nonce, the webview would ignore `'unsafe-inline'` and block every style attribute. Scripts keep Tauri's hashing.
  - `connect-src`: `'self'`, `ipc:`, `http://ipc.localhost`, `asset:`, `http://asset.localhost` (wavesurfer fetches audio from the asset protocol), `blob:`, `data:` (image re-encoding fetches a data URL), and the hosts reached with the webview's own `fetch`: `https://api.anarlog.so` (sync, billing, API client), `https://anarlog.so` (resource lists), `https://static.anarlog.so`, `https://api.github.com` and `https://raw.githubusercontent.com` (changelog, GitHub repo search), `https://*.supabase.co`, `https://*.sentry.io` (Sentry browser SDK). `devCsp` adds http and ws on `localhost` / `127.0.0.1` for Vite, HMR and the local API, and allows inline scripts for React Fast Refresh.
  - `img-src` adds `https:`. `media-src`, `font-src` and `worker-src` stay on local schemes. `frame-src`, `object-src` and `form-action` are `'none'`.
- **Gaps (why PARTIAL):**
  - `img-src https:` lets injected script leak small amounts of data through image URLs. It's there because avatars come from arbitrary hosts (team avatars, GitHub task assignees, Gravatar) and notes can embed remote images. A host list would break those.
  - `VITE_API_URL` / `VITE_SUPABASE_URL` are build-time settings. A build pointing at another host needs the CSP updated. The production API value comes from `.github/workflows` (`https://api.anarlog.so`); the Supabase project URL is a secret, so `*.supabase.co` is allowed. GUESS: the project is on `supabase.co`.
  - Not verified at runtime. The screens most at risk are listed under "Phase 1 runtime check".

### A2.5 Per-window capabilities: PARTIAL

`capabilities/default.json` (`"windows": ["*"]`) is replaced by one file per window kind. No `"*"` entry remains.

| File                             | Windows             | Grants                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `capabilities/main.json`         | `main`, `note-*`    | the previous full set minus `hooks:default` (data, files, secrets, http, shell open)                                                                                                                                                                                                                                                                                                                                                   |
| `capabilities/composer.json`     | `composer`          | the main set minus `fs2`, `export`, `importer`, `attachment-sync`, `local-api` (webhooks), `accounts`, `dialog`, `shell:allow-open`, `updater`/`updater2`, `autostart`, `relay`, `mcp`, `js`, `sfx`, `overlay`, `dock`, `messenger`, `audio-priority`, `shortcut`, `path2`, clipboard read, `os:allow-hostname`, and the CloudSync activity commands. It keeps db, tantivy, store2, AI and http because the quick-Ask chat needs them. |
| `capabilities/floating-bar.json` | `floating-bar`      | `core:event`/`core:window` defaults, theme, zoom, `core:app:default`; `windows:allow-floating-bar-current-state` and the webview health commands; `db:allow-wait-until-ready`, `get-startup-status`, `subscribe`, `unsubscribe`, `execute`; `local-auth:allow-available` / `allow-authenticate`; `icon:allow-set-dock-icon`; `tracing:allow-do-log`                                                                                    |
| `capabilities/live-caption.json` | `live-caption`      | same as the floating bar with `windows:allow-live-caption-current-state`                                                                                                                                                                                                                                                                                                                                                               |
| none                             | `dictation-overlay` | no IPC at all (`dictation.html` has no script; Rust updates it with `eval`)                                                                                                                                                                                                                                                                                                                                                            |

How the overlay lists were derived: the floating bar and live caption load the whole bundle (`main.tsx` → `MainAppLayout` → `/app` route), so the shared boot path decides most of what they call. `LongLoadGate` (`db:wait_until_ready`, `get_startup_status`) and `useConfigValue` / `useSettingsReady` (`db:subscribe` on `app_settings`) block rendering if denied, so they are granted. Everything else on that path (AuthProvider token reads, billing, the workspace mirror, deep-link draining, detect notifications) catches its errors and is **deliberately not granted**; the overlay then behaves as signed out. The route components themselves only call `windows:*_current_state` and use events.

- **Gaps (why PARTIAL):**
  - The overlays still get `db:allow-execute` / `subscribe`, so a compromised overlay can read app tables (under the authorizer). Fix in Phase 2: a narrow settings-read command, or skip `AuthProvider`/`BillingProvider`/`SharedWorkspaceMirror`/`ListenerProvider` detect handling for non-main labels.
  - On macOS the `floating-bar` and `live-caption` webviews are never created (native Swift panels; live caption is disabled on every platform), so these two files only matter on Windows/Linux.
  - The composer list comes from a static import trace, not a runtime trace. GUESS: anything it touches that was dropped fails with a logged ACL error, not a crash.
  - App-level commands in `apps/desktop/src-tauri/src/commands.rs` have no app manifest, so the ACL doesn't gate them in any window (unchanged).

### A3.7 `get_env`: EXISTS

- `apps/desktop/src-tauri/src/commands.rs:46` `WEBVIEW_ENV_ALLOWLIST` is empty: no frontend code calls `getEnv` (grep of `apps/desktop/src`, `packages`, `plugins/*/js`), so every key reads as unset. `get_env` (`:58`) keeps its signature (returns `""` for "unset") so the generated bindings don't change. Test: `env_tests::get_env_never_returns_keys_outside_the_allowlist`.

### A3.6 Paths from the webview: PARTIAL

- **Check** (`plugins/fs2/src/grants.rs`, `plugins/fs2/src/ext.rs:20-84`): a path must be absolute and contain no `..`; the longest existing ancestor is canonicalised (symlinks resolved) and the rest appended. It is accepted if
  - it is under the vault or the app data directory, and for writes has a document extension (`md`, `txt`, `org`, `pdf`; `grants.rs:19`); or
  - the user picked it moments ago in a native dialog opened by Rust. Picks are recorded per path, consumed on first use, and expire after 10 minutes (`grants.rs:16`).
- **New commands:** `fs2:pick_save_path` (`plugins/fs2/src/commands.rs:58`) and `fs2:pick_open_paths` (`:95`) open the native dialog in Rust and record the pick. They come with `fs2:default`, which only `main.json` grants, so only main and note windows can open them.
- **Applied to:**
  - `fs2:write_text_file` (`plugins/fs2/src/commands.rs:27`, `ext.rs:77`);
  - `export:export` (`plugins/export/src/commands.rs:17`);
  - `importer:read_text_files` (`plugins/importer/src/commands.rs:87`);
  - `fs-sync:audio_import` (`plugins/fs-sync/src/commands.rs:324`). Staged recording imports (CLI/MCP `recording_import`) live in the vault, so the recording-import worker still works.
  - `fs2:read_text_file` and `fs2:remove` keep their vault-only rule, now with the stricter resolver: before, a not-yet-existing path such as `<vault>/../x` passed the `starts_with` check.
- **Frontend callers switched** from the JS dialog plugin to the Rust pickers: note export (`export-modal.tsx:438`; it now always shows a save dialog prefilled with the export folder, where before only App Store builds did), the E2EE recovery-key download (`e2ee-setup.tsx:121`; now a save dialog prefilled with Downloads), meeting-export import (`imports/screen.tsx:201`) and audio/transcript upload (`useUploadFile.ts:397`; the picker no longer opens in Downloads by default).
- **Gaps (why PARTIAL):**
  - The dialog returns the raw path, not an opaque token as A3.6 asks. The path is only honoured once, so a token would add little, but the rule's wording isn't met.
  - Other commands still take a webview path and aren't checked: `transcription:parse_subtitle`, `fs-sync:audio_source_metadata`, `opener2:open_path` / `reveal_item_in_dir`, and `attachment-sync` / `session` commands that take paths (not audited in this phase). `parse_subtitle` reads any `.srt`/`.vtt`-style path the webview names. GUESS: low impact, since it only returns parsed subtitle tokens.
  - The `export_directory` setting is still written by the webview; it is now only a dialog default, not a write permission.
  - The JS `dialog:default` permission stays for the folder pickers that don't feed a gated command (export location, local model file, automations folder).

### opener2 `open_with`: EXISTS

- `plugins/opener2/src/ext.rs:12` `OPEN_WITH_ALLOWLIST` is empty and `checked_open_with` (`:14`) rejects any non-null `open_with` for `open_url` and `open_path`. Every frontend caller passes `null`. Test: `open_with_is_rejected_unless_allowlisted`.
- Still open (not in this phase): `open_path` opens any path with its default app, and `open_url` takes any scheme. With `write_text_file` now limited to document types in the app roots or a dialog pick, the write-then-open chain from the audit no longer lands an executable. GUESS: a `.md`/`.txt` opened with its default app can't run code.

### B2.2 Webhook URLs: PARTIAL

- `create_endpoint` (`plugins/local-api/src/dispatch.rs:140`) now parses the URL (`parse_webhook_url`, `:331`): `https://` only, no credentials, no `localhost` / `*.localhost`. Before inserting it resolves the host (`resolve_public_addrs`, `:391`) and fails if any address isn't public (`is_public_ip`, `:352`): loopback, private (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10), CGNAT 100.64/10, unspecified, broadcast, multicast, documentation and benchmark ranges, 0/8, 240/4, unique-local fc00::/7, NAT64 64:ff9b::/96, and IPv4-mapped IPv6 forms of all of these.
- `deliver_once` (`:416`) repeats the check on every delivery, because a row can be written straight into `webhook_endpoints` through webview SQL, and DNS can change after creation. It pins the checked addresses with `resolve_to_addrs` (`:443`) so the connection can't be rebound, and doesn't follow redirects (`:441`).
- Tests: `endpoint_creation_rejects_bad_urls_and_unknown_events` (13 blocked URLs), `public_addresses_are_allowed`.
- **Gaps (why PARTIAL):** there is no "user confirms" override for LAN targets (B2.2 allows one); a self-hosted receiver on the LAN or on this Mac no longer works. The webhook secret is still plaintext in SQLite (C3, unchanged).

### A3.3 HTTP scope: PARTIAL

- `capabilities/main.json` and `capabilities/composer.json` (`http:default`, `main.json:100`) now allow only:
  - app: `https://api.anarlog.so`, `https://*.supabase.co`;
  - subscription sign-in: `claude.ai`, `platform.claude.com`, `auth.openai.com`, `auth.x.ai`, `github.com`, `api.github.com` (also GitHub repo search);
  - every built-in LLM and STT provider base URL in `apps/desktop/src/settings/ai/llm/shared.tsx` and `settings/ai/stt/shared.tsx` (39 hosts);
  - cloud providers whose host depends on the account: `*.openai.azure.com`, `*.services.ai.azure.com`, `*.cognitiveservices.azure.com`, `aiplatform.googleapis.com`, `*-aiplatform.googleapis.com`, `api.cloudflare.com`, `gateway.ai.cloudflare.com`, `bedrock-mantle.*.api.aws`, `bedrock-runtime.*.amazonaws.com`, `*.asr.api.speechmatics.com`;
  - loopback: `http://localhost:*`, `http://127.0.0.1:*` and their `https` forms.
  - Calendar (Google/Microsoft), updates and analytics are called from Rust, not through the `http` plugin, so they aren't listed. The overlays get no `http` permission at all.
- Every pattern was checked with the same URL-pattern parser the plugin uses: all parse, provider/loopback URLs match, and `https://evil.com`, `http://api.openai.com`, LAN IPs, `api.openai.com.evil.com` and `169.254.169.254` don't. Test `capabilities_are_per_window_with_a_host_allowlist` (`apps/desktop/src-tauri/src/lib.rs`) fails on `**`, `*.*.*.*`, plain-http non-loopback entries, or a `"*"` window.
- **The custom-URL trade-off:** most providers let the user change the base URL under Advanced, and the `custom`, Azure, Vertex, Bedrock and Cloudflare providers need one. Allowing them all statically would mean `https://**`. Instead:
  - `scopedFetch` (`apps/desktop/src/ai/provider-fetch.ts:14`, used by `providerFetch` and the enterprise-capture client) catches the plugin's "url not allowed on the configured scope" error, calls `misc:allow_http_origin`, and retries once.
  - `plugins/misc/src/http_origins.rs`: Rust accepts only `https://` origins, or `http://` to a private/link-local IP or a `.local` name (`approvable_origin`, `:22`); shows a native warning dialog naming the exact origin (`request`, `:110`); on "Allow" adds a runtime capability scoped to `origin/*` for `main`, `note-*` and `composer` (`:18`, `:88`) and stores the origin in `approved-http-origins.json` in the app data directory, re-applied at startup (`restore`, `:100`). No webview command can write that file (fs2 writes in app data are limited to document extensions).
  - Remaining risk: injected script can trigger the dialog with an attacker host and hope the user clicks Allow. The dialog text says to allow it only if the user typed it. There's no UI yet to list or revoke approved origins (delete the file to reset).
- **Gaps (why PARTIAL):**
  - LAN model servers over plain `http://` with a hostname (not an IP or `.local`) are no longer reachable; previously `http://**` allowed them.
  - `VITE_AI_API_URL` / `VITE_ENTERPRISE_API_URL` are build settings; a build that points them elsewhere hits the approval dialog (enterprise) or fails (hosted AI). GUESS: production uses `api.anarlog.so` for both.
  - STT batch/live traffic runs in Rust (`owhisper-client`) with no host allowlist; A3.3 only covers the webview's `http` plugin.
  - Not verified at runtime that a runtime-added capability takes effect for the `http` plugin without a restart. Reading `tauri-2.11.5/src/ipc/authority.rs:150-200`, `add_capability` updates the command scopes the plugin reads on each call (`dynamic-acl` is a default feature). GUESS: it works live.

### A3.2 Asset scope: EXISTS

- Static scope is now `["$APPCACHE/**"]` (`apps/desktop/src-tauri/tauri.conf.json:49`): shared-note previews and attachment caches (`plugins/attachment-sync/src/runtime/cache.rs`) and audio peaks live there. `**/*` and `$APPDATA/**` are gone.
- The vault is user-configurable (`crates/storage/src/vault/path.rs:28-43`, resolved once at startup by `plugins/settings/src/lib.rs:36`), so `allow_vault_sessions_in_asset_scope` (`apps/desktop/src-tauri/src/lib.rs:754`, called from `setup` at `:418`) adds `<vault>/sessions` (and its canonical form) recursively. That covers session audio, recovery chunks and attachments (`crates/fs-sync-core/src/lib.rs:31,195`). Changing the vault takes effect after the relaunch the app already does.
- Only `sessions/` is added, not the whole vault: a custom vault can be the user's Obsidian folder, and the default base also holds `settings.json` and `app.db`.
- GUESS: nothing else the UI renders through `convertFileSrc` lives outside these two roots (checked: session audio, attachments, shared-note attachment and preview caches, meeting screenshots).

### Hooks feature: REMOVED

- Deleted `plugins/hooks` (`tauri-plugin-hooks`, JS package `@anlg/plugin-hooks`) and `crates/hooks` (`anlg-hooks`), their workspace entries in `Cargo.toml`, the app dependency in `apps/desktop/src-tauri/Cargo.toml`, the registration in `apps/desktop/src-tauri/src/lib.rs`, the `hooks:default` capability, the `--exclude tauri-plugin-hooks` line in `.github/workflows/desktop_ci.yaml`, the `@anlg/plugin-hooks` dependency in `apps/desktop/package.json` and `pnpm-lock.yaml`.
- Removed both `runEventHooks` callers (`beforeListeningStarted`, `afterListeningStopped`) from `apps/desktop/src/store/zustand/listener/general-live.ts`; starting a recording no longer needs `settings:vault_base`.
- There was no settings-schema entry or settings UI for `hooks` (the plugin read the raw `hooks` key from `settings.json`). A leftover key is ignored.
- Removed the "shell hooks" claim from `apps/web/content/articles/char-vs-meetily.mdx`.

### Phase 1 runtime check

None of Phase 1 was run in the app; only unit tests, `tsc`, vitest and `cargo check` ran. Screens and flows most at risk, to check for CSP violations (Web Inspector console) and ACL errors (`not allowed` in `app.log`):

1. **App boot in every window** (main, note window, composer): `withGlobalTauri: false`, the CSP, and the Zod jitless switch all apply at startup. A blank window means a blocked script or style.
2. **Composer (quick Ask)**: lost several plugins (`capabilities/composer.json`). Ask a question with each kind of provider; open a note link from the answer.
3. **Ask / enhance with a custom or Azure/Vertex/Bedrock/Cloudflare base URL**: should show the "Allow connection?" dialog once, then work. Built-in providers should work with no dialog. Check model listing and key verification in Settings → AI.
4. **Local models** (Ollama, LM Studio on `127.0.0.1`/`localhost`): should work with no dialog. A LAN server by IP shows the dialog; by plain hostname over http it no longer works.
5. **Sign-in and sync**: Supabase auth refresh, billing/trial dialogs, team workspace mirror, E2EE setup (recovery-key download now opens a save dialog).
6. **Note export** (PDF/MD/TXT/Org): always opens a save dialog now. **Meeting import from files** and **audio/transcript upload**: use the new Rust file picker. **Recording-import jobs** from the CLI/MCP.
7. **Audio playback and waveform** on a past meeting, **attachments/screenshots** in notes, **shared-note previews**: asset scope is now cache + `<vault>/sessions`. A custom vault outside the default location is the riskiest case.
8. **Images and avatars** (team avatars, GitHub assignees, Gravatar), **code blocks in Ask answers** (Shiki), **Mermaid diagrams** in Streamdown: rely on `img-src https:`, inline styles, and no wasm.
9. **Changelog / What's new** (GitHub raw content), **resource lists** (`anarlog.so`), **Sentry** error reporting if enabled.
10. **Floating bar / live caption** (Windows/Linux only): boot with the reduced overlay capability.
11. **Developers → Webhooks**: creating `http://` or LAN URLs is now refused; test delivery to a public `https://` receiver.
12. **Dev mode** (`pnpm dev:desktop`): `devCsp` must allow Vite HMR and the relay shim.

## Phase 2a changes (central check, command guard, Rust-held lock), 2026-11-16

Nothing below was run in the app; see "Phase 2a runtime check".

### A1 central check: PARTIAL

- `crates/aspc` (no dependencies by default; `serde`, `specta` and `tauri` are features):
  - `Role`, `DataType`, `Action` (`crates/aspc/src/lib.rs:24`, `:60`, `:92`). `Role::parse` accepts only the four lowercase names; anything else is unknown (`:36`).
  - `AccessContext {actor_id, role, workspace_id, device_id, window, unlocked}` (`:119`).
  - `authorize` / `authorize_resource` (`:237`, `:250`) check, in order: lock, workspace, role, matrix (`:288`), window policy (`window_allows`, `:367`). Admin on Keys gets a `PresenceOnly` grant; `Grant::require_full` turns it into a denial for commands that return a value (`:164`).
  - Member ownership: `Ownership::{Unknown, OwnedBy, SharedToActor}` (`:140`). Command-level checks pass `Unknown`, so there a Member may only `Create`. This is documented in ASPC_BASE §0.
  - Stable codes (`DenyCode`, `:176`).
  - Tests: `owner_may_do_everything`, `admin_matrix`, `member_matrix`, `viewer_reads_content_only` (A1.4), `lock_workspace_and_unknown_role_deny`, `windows_limit_owner`, `denial_codes_are_stable`.
- A1.2 test: `crates/aspc/tests/role_comparisons.rs`. A bare `role ===` is not matched, because chat messages use `role === "user"`; the patterns are listed in the file.
  - **Allowlisted (existing code, each with its reason in the file):**
    - `apps/desktop/src/settings/team/index.tsx` and `settings/team/client.ts`;
    - `contacts/organization-details.tsx`;
    - `session-sharing/source.ts`;
    - `shared-notes/index.tsx` and `shared-notes/cache.ts`;
    - `resource-sharing/{share-dialog.tsx,library.tsx,client.ts}`;
    - `folders/folder-editor.tsx`;
    - `auth/cloudsync-credentials.ts` and `auth/cloudsync-workspace-keys.ts`;
    - `crates/db-app/src/cloudsync/projection.rs`;
    - `crates/api-sync/src/routes/cloudsync_credentials/{grants.rs,projection.rs}`.
  - All of these mirror hosted roles for display or validate server responses. `session-sharing/source.ts` is the one that decides something locally, and should move behind `aspc` with Phase 2b.
- **Gaps:** no generated TS mirror; signed-in team roles are not read.

### A1.3 local access context: EXISTS

- Built in Rust before any window exists (`apps/desktop/src-tauri/src/lib.rs:187`, managed at `:258`) by `plugins/local-auth/src/access.rs:138`:
  - **actor id:** created once and kept in the Keychain through `local_accounts::KeychainStore`, service `<bundle id>.access`, item `local-actor-id` (`access.rs:23`). If the Keychain doesn't answer within 3 s (for example a dev build waiting on an access prompt), that launch uses an `ephemeral-…` id and logs `aspc_actor_id_ephemeral`.
  - **role:** `Owner`.
  - **workspace:** the first `workspaces` row with `kind='personal'` (preferring one owned by the actor), else a new row `local-workspace-<uuid>` owned by the actor (`access.rs:64`). No schema change. If the insert fails the workspace id is empty and every check denies (fail closed).
  - **device id:** `anlg_host::fingerprint()`, the hashed machine id the app already uses for analytics.
- Read-only command `local-auth:access_context` returns the non-secret fields (`plugins/local-auth/src/commands.rs:49`).
- Tests in `plugins/local-auth/src/access.rs`: `actor_id_is_created_once`, `personal_workspace_is_reused_or_created`, `lock_setting_round_trip_and_legacy`, `bootstrap_starts_locked_when_the_setting_is_on`.
- GUESS: after sign-in, the CloudSync workspace projection (`crates/db-app/src/cloudsync/projection.rs:436`) replaces the `workspaces` table, so the next launch reuses the account's personal workspace instead of the local row. Not tested.

### A2.1–A2.3 command registry and guard: PARTIAL

- Registry `crates/aspc/src/commands.rs:113`: 460 commands (455 before Phase 2a, plus 5 new `local-auth` ones):
  - 184 `Public`, each with a reason;
  - 34 guarded;
  - 8 guarded without the lock check;
  - 234 `Pending`.
- `tests/command_registry.rs`:
  - `registry_matches_real_commands` parses every `collect_commands!` / `generate_handler!` list in `plugins/*/src/lib.rs` and the app's `lib.rs`. The autogenerated permission `.toml` files are stale in several plugins, so they are not used.
  - `guarded_commands_call_the_guard` checks that each guarded command's body calls `aspc::require` (or `require_ignoring_lock`).
- Guard (`crates/aspc/src/tauri_guard.rs:46`, `:65`): reads `Arc<AspcState>` from Tauri state and the caller's window label. Without the state it fails closed (`aspc.no_context`).
- **Guarded, lock enforced:**
  - `db:list_meetings`, `get_meeting`, `get_meeting_transcript`, `get_recurring_meeting_history`;
  - `export:export`;
  - `fs-sync:audio_import`, `audio_import_data`;
  - `fs2:read_text_file`, `remove`, `write_text_file`;
  - `importer:begin_connected_import`, `complete_connected_import`, `read_text_files`, `sync_connected_import`;
  - `local-api:create_webhook`, `delete_webhook`, `export_meeting_markdown`, `get_cloud_snapshot`, `list_cloud_snapshot_ids`, `list_webhooks`, `prepare_drive_markdown`, `set_webhook_active`, `test_webhook`;
  - `local-auth:set_app_lock_enabled`;
  - `session:soft_delete_session`, `restore_deleted_session`;
  - `settings:save`;
  - `store2:get_secret` (full grant required), `set_secret`, `delete_secret`, `repair_keychain_access`;
  - `tantivy:search`;
  - `transcription:export_to_vtt`, `render_session_transcript`.
- **Guarded without the lock check (role and window only):**
  - `db:execute`, `execute_proxy`, `execute_transaction`, `subscribe`: the SQL authorizer enforces the lock per statement.
  - `auth:get_item`, `set_item`, `remove_item`, `clear`: background token refresh runs while relocked, and a denial reads as "no session" (`apps/desktop/src/auth/client.ts:21-27`), which would sign the user out.
- `auth:begin_refresh` / `finish_refresh` were reclassified `Public`: they only coordinate refresh timing between windows.
- **Not guarded yet (`Pending`):**
- `accounts` (3): `connect_account`, `disconnect_account`, `list_accounts`
- `agent` (2): `install_cli`, `uninstall_cli`
- `analytics` (1): `set_disabled`
- `app` (13): `get_account_full_name`, `get_dismissed_toasts`, `get_pinned_tabs`, `get_recently_opened_sessions`, `get_tinybase_values`, `install_agent_skill`, `install_embedded_cli`, `is_crash_reporting_enabled`, `set_crash_reporting_enabled`, `set_dismissed_toasts`, `set_onboarding_needed`, `set_pinned_tabs`, `set_recently_opened_sessions`
- `attachment-sync` (19): `begin_attachment_download`, `begin_shared_upload_operation`, `cleanup_shared_upload`, `cleanup_transfer_cache`, `clear_shared_attachment_preview_scopes`, `clear_shared_attachment_scope`, `commit_delete_guard`, `describe_upload`, `download_and_restore`, `download_shared_attachment`, `prepare_delete_guard`, `prepare_shared_upload`, `prepare_upload`, `read_shared_upload_range`, `read_upload_range`, `reconcile_delete_guards`, `remove_shared_attachment`, `shared_attachment_path`, `validate_shared_upload`
- `audio-priority` (4): `get_input_priorities`, `get_output_priorities`, `save_input_priorities`, `save_output_priorities`
- `auth` (1): `get_account_info`
- `calendar` (21): `apply_calendar_inventory`, `apply_contact_enhancement`, `create_event`, `create_human`, `create_organization`, `list_calendars`, `list_connection_ids`, `list_events`, `merge_humans`, `reorder_pinned_contacts`, `save_personal_contact`, `set_calendar_enabled`, `soft_delete_contact`, `sync_calendar_connection_events`, `toggle_contact_pin`, `tombstone_calendar_connection`, `update_contact_avatar`, `update_human`, `update_human_contact_summary`, `update_ignored_calendar_item`, `update_organization`
- `db` (28): `apply_session_ingest`, `begin_cloudsync_activity`, `bind_cloudsync_account`, `cleanup_legacy_files`, `cleanup_recording_imports`, `configure_cloudsync`, `configure_cloudsync_token`, `configure_e2ee_replica`, `connect_local_library`, `create_e2ee_identity`, `end_cloudsync_activity`, `get_cloudsync_status`, `get_e2ee_identity_status`, `get_legacy_cleanup_status`, `get_legacy_import_report`, `get_or_create_e2ee_device_identity`, `import_e2ee_device_enrollment`, `import_e2ee_identity`, `inspect_e2ee_recovery_key`, `run_legacy_import`, `seal_e2ee_recovery_key_for_device`, `seal_workspace_e2ee_key_for_recipients`, `start_cloudsync`, `stop_cloudsync`, `suspend_cloudsync`, `suspend_cloudsync_after_auth_loss`, `suspend_cloudsync_for_sign_out`, `sync_cloudsync_now`
- `detect` (10): `capture_meeting_chat_messages`, `capture_meeting_screen_share`, `inspect_meeting_accessibility`, `send_meeting_chat_message`, `set_ignored_bundle_ids`, `set_included_bundle_ids`, `set_mic_active_threshold`, `set_respect_do_not_disturb`, `start_active_speaker_tracking`, `stop_active_speaker_tracking`
- `dictation` (3): `start_recording`, `start_system_recording`, `stop_recording`
- `fs-sync` (31): `attachment_list`, `attachment_read`, `attachment_remove`, `attachment_save`, `audio_copy`, `audio_delete`, `audio_delete_orphaned_expired`, `audio_exist`, `audio_has_speech`, `audio_metadata`, `audio_path`, `audio_peaks`, `audio_source_metadata`, `chat_dir`, `create_folder`, `delete_folder`, `delete_session_folder`, `entity_dir`, `folder_attachment_list`, `folder_attachment_read`, `folder_attachment_remove`, `folder_attachment_save`, `list_folders`, `load_session_content`, `move_session`, `read_document_batch`, `rename_folder`, `scan_and_read`, `session_dir`, `write_document_batch`, `write_json_batch`
- `local-api` (1): `dispatch_event`
- `misc` (1): `allow_http_origin`
- `session` (23): `add_session_participant`, `catalog_folder_material`, `catalog_note_attachment`, `catalog_session_audio`, `create_session`, `create_session_for_event`, `delete_folder_catalog`, `ensure_folder_catalog`, `mark_session_audio_absent`, `mark_session_audio_transcription_complete`, `move_session_contents`, `persist_chat_session_proposal`, `remove_session_participant`, `rename_folder_catalog`, `resolve_session_conflict`, `resolve_session_conflicts`, `set_attachment_cloud_sync_enabled`, `set_session_proposal_status`, `tombstone_folder_material`, `tombstone_session_audio`, `update_folder_icon`, `update_folder_instructions`, `update_folder_workspace`
- `settings` (1): `load`
- `store2` (7): `get_bool`, `get_number`, `get_str`, `save`, `set_bool`, `set_number`, `set_str`
- `tantivy` (5): `add_document`, `reindex`, `remove_document`, `update_document`, `update_documents`
- `template` (4): `apply_session_content_corrections`, `get_template_source`, `save_generated_summary`, `save_generated_title`
- `todo` (11): `complete_todo`, `create_todo`, `delete_todo`, `fetch_todos`, `github_issue_comments`, `github_issue_detail`, `github_issue_state`, `linear_list_teams`, `linear_list_tickets`, `list_todo_lists`, `read_path`
- `tracing` (1): `log_content`
- `transcription` (43): `acknowledge_capture_audio_chunk`, `acknowledge_capture_audio_cleanup_status`, `acknowledge_completed_transcription`, `acknowledge_stopped_capture`, `attribute_session_speakers`, `clear_capture_audio_saved`, `clear_capture_lifecycle_marker`, `delete_processed_session_audio`, `delete_transcribed_capture_audio`, `delete_voice_enrollment`, `extract_voiceprint_candidates`, `finish_voice_enrollment`, `flush_live_transcript`, `get_capture_audio_cleanup_status`, `get_capture_audio_gaps`, `get_capture_lifecycle_marker`, `get_capture_snapshot`, `get_completed_transcription`, `get_stopped_capture`, `improve_voice_enrollment_from_segment`, `list_capture_audio_chunks`, `list_capture_lifecycle_markers`, `list_capture_recoveries`, `list_stopped_captures`, `list_transcription_sessions`, `mark_capture_audio_saved`, `promote_voiceprint_candidates`, `reconcile_refined_speaker_clusters`, `refine_batch_transcript`, `release_live_transcript`, `retry_audio_capture`, `save_batch_transcript`, `save_capture_lifecycle_marker`, `set_capture_paused`, `start_capture`, `start_transcription`, `start_voice_enrollment`, `stop_capture`, `stop_capture_for_session`, `stop_transcription`, `update_capture_config`, `update_capture_credentials`, `voice_enrollment_status`
- `updater2` (1): `set_automatic_updates_enabled`
- Why these are pending: most run in the background while the window is hidden (capture pipeline, calendar sync, vault mirror, search indexing, CloudSync, attachment sync, webhook dispatch, scheduled auto-start creating sessions). Guarding them with the lock would break recordings and sync after a relock. They need a background-job context (a Rust-side caller that is not the webview) before the guard can go in.
- Test for A2.3: `guard_denies_locked_viewer_and_wrong_window` in `crates/aspc/src/state.rs` runs every registry `(data, action)` through the guard's logic with a locked context, a Viewer and the dictation-overlay window. It tests the guard function, not each command end to end.

### A2.4 SQL authorizer: EXISTS (was PARTIAL)

- Per-table mapping, `table_data_type` (`crates/db-core/src/webview_sql.rs:65`):
  - transcripts → Transcript;
  - recording-import jobs and voiceprints → Recording;
  - `action_items` → Actions;
  - settings, templates, calendars, workspaces → Settings;
  - `api_keys`, `webhook_endpoints`, `e2ee_*` → Keys;
  - everything else → Notes.
- Each column read is `Read` and each insert, update or delete is `Edit`, checked with `aspc::authorize` against the context the app sets at startup (`set_context`, `:118`; wired in `apps/desktop/src-tauri/src/lib.rs:788`). Viewer writes are denied, and a Viewer can't read settings or keys.
- Lock: driven by the Rust lock (`AspcState::on_lock_change` → `set_webview_sql_locked`). While locked, only `SELECT` of `app_settings`, `synced_preferences` and the schema table is allowed (`decide_locked`, `:157`). The early refusal in `AuthorizedConnection::acquire` was removed so that this path works.
- Tests: `denies_everything_but_lock_screen_settings_when_locked`, `applies_the_role_per_table`, `maps_tables_to_data_types`, plus the Phase 1 tests.
- **Gaps:**
  - no window per statement (the `db:*` guard checks it instead);
  - no row filter;
  - `webhook_endpoints.secret` is still readable by an unlocked Owner's webview;
  - CLI/MCP don't use the authorizer.

### A2.5 window policy in Rust: added

- `aspc::window_allows` (`crates/aspc/src/lib.rs:367`) repeats the capability split in Rust for guarded commands.
- **The composer gets no Keys**, as specified, so `store2:get_secret` and `auth:get_item` are denied there.
  - GUESS: the composer reads provider keys through `getProviderApiKey` (`apps/desktop/src/settings/providers.ts:439`) and the sign-in session through `auth:get_item`. If so, composer Ask with a BYOK key fails, and the composer behaves as signed out for hosted AI. Check by hand (Phase 2a runtime check, item 6).
- Capabilities: `floating-bar.json` and `live-caption.json` gained `local-auth:allow-app-lock-state` and `allow-unlock-app`, so their lock gate can read the Rust state.

### A4.2–A4.4 Rust-held lock: PARTIAL

- State machine `LockMachine` (`crates/aspc/src/lock.rs`), held in `AspcState` (`crates/aspc/src/state.rs:10`). The app starts locked when the `lock_app` setting is on. Rust reads it from `app_settings`, falling back to the legacy settings document (`plugins/local-auth/src/access.rs:98`).
- Unlock: `local-auth:unlock_app` (`plugins/local-auth/src/commands.rs:92`) runs `LAContext.evaluatePolicy` itself (`plugins/local-auth/src/app_lock.rs:84`), with a fixed prompt reason.
  - A lock during the prompt wins (epoch check).
  - If authentication is unavailable it returns `aspc.auth_unavailable` and stays locked (A4.3). `plugins/local-auth/src/macos.rs` now reports "unavailable" instead of "failed" for that case.
- Settings toggle: `local-auth:set_app_lock_enabled` runs device authentication in Rust and writes `lock_app` (`app_lock.rs:98`). The privacy screen calls it instead of writing the setting through SQL (`apps/desktop/src/settings/privacy/index.tsx`).
- Relock (`apps/desktop/src-tauri/src/lib.rs:788`, `AppLockExt::request_relock` at `app_lock.rs:160`):
  - **main window hidden:** the webview flushes pending writes and asks Rust to lock; Rust locks anyway after 5 s if the window is still hidden;
  - **Mac sleep:** through a new Rust hook, `tauri_plugin_detect::on_sleep_state_changed` (`plugins/detect/src/handler.rs:39`);
  - both wait while the capture state is not `Inactive`, retrying every 15 s;
  - **idle timeout:** not built (no setting exists).
- Frontend:
  - `apps/desktop/src/lock/store.ts` mirrors Rust (the `appLockChanged` event and `app_lock_state`).
  - `lock/gate.tsx` shows the lock screen from that state and asks Rust to unlock. After a relock it restarts every live query (`restartLiveQueries`, `packages/db-react/src/index.ts:33`) and invalidates TanStack queries.
  - Main-window startup writes wait for the unlock (`apps/desktop/src/main.tsx`).
- Tests:
  - `crates/aspc/src/lock.rs` (7) and `state.rs`;
  - `plugins/local-auth/src/app_lock.rs`;
  - `apps/desktop/src/lock/{store,gate}.test.*` and `settings/privacy/index.test.tsx`;
  - `packages/db-react/src/index.test.ts`.
- **Gaps:**
  - `Pending` and lock-exempt commands answer while locked;
  - post-meeting work that finishes after the relock (enhance reading a provider key, webhook dispatch, calendar sync writes) fails until the next unlock;
  - batch transcription of imported audio does not hold off the relock;
  - CLI/MCP ignore the lock.

### Phase 2a runtime check

To check by hand in the running app (look in `app.log` for `aspc.` errors, and in Web Inspector for failed `db:` calls):

1. **Lock off (the default):** the app opens with no prompt; notes, search, export, settings and AI keys work.
2. **Turn the lock on** in Settings → Privacy: a Touch ID prompt ("change lock settings"); the app stays open. Quit and reopen: the lock screen comes first, then Touch ID ("open"), then all data loads.
3. **Cancel the prompt** on the lock screen: the app stays locked; "View" prompts again.
4. **Close the main window** (it hides) and reopen it from the Dock: lock screen, prompt, and the data reloads (open a note, check the sidebar list and search).
5. **Sleep the Mac** with the app open, then wake it: lock screen.
6. **Composer (quick Ask):** with the lock off, ask with a BYOK provider and with the hosted model. Expected to fail with `aspc.window_denied` on `store2:get_secret` / `auth:get_item` (see A2.5). Decide whether to keep that.
7. **Recording while hidden:** start a meeting, close the main window, talk for a minute, then stop from the tray. The transcript and note must be saved, and the app should lock only after the capture ends.
8. **Enhance after a hidden recording** (lock on): check whether the summary is written or fails while locked.
9. **Turn the lock off** again: it needs Touch ID; after a relaunch there is no lock screen.
10. **Lock-out risk:** with the lock on, if Touch ID and the password prompt are both unavailable, the app stays locked with a message. Recovery outside the app: quit it and run `sqlite3 "<app data>/app.db" "UPDATE app_settings SET value_json='false' WHERE id='lock_app'"`.
11. **Note windows** and **overlays** (the Windows/Linux floating bar and live caption) follow the same lock state.
12. **Sign-in refresh while locked:** leave the app hidden and locked for over an hour while signed in. After unlocking you must still be signed in.

## Phase 2b changes (C1 workspace isolation), 2026-11-16

Nothing below was run in the app; see "Phase 2b runtime check".

### C1 local identity: changed from Phase 2a

- The data model already had a local workspace: the CloudSync binding (`app_settings.cloudsync_workspace_binding`), a UUID made on first run that becomes the account id when an account claims the device. Rust create paths already write it (`insert_session`), and the claim (`crates/db-app/src/cloudsync/binding.rs:152`) moves `''` rows and that workspace's rows into the account, and **fails with `ForeignWorkspace` if any domain row has another workspace id**.
- Phase 2a's context used a Keychain actor id and a new `local-workspace-<uuid>` row. Backfilling rows with those ids would have broken sign-in (the claim) and the owner rekey. So the context now uses the data model's identity: `anlg_db_app::local_content_identity` (`crates/db-app/src/content_identity.rs:92`): workspace = binding workspace; owner/actor = the active local library's workspace, else the binding workspace (the rule `insert_session` uses). The Keychain actor item is no longer read.
- Startup: `plugins/local-auth/src/access.rs:87` reads it if the schema is ready; otherwise the workspace is empty and every check denies. This also fixes a Phase 2a gap: on a fresh install the schema is not migrated when the context is built, so the first launch had no workspace. The db plugin sets the identity once the schema is ready (`plugins/db/src/lib.rs:395` → `runtime.rs:624`, `AspcState::set_identity`, `crates/aspc/src/state.rs:74`) and again after a claim (`runtime.rs:1307`, `:1342`). The webview SQL context follows through `on_identity_change` (`apps/desktop/src-tauri/src/lib.rs`, `wire_app_lock`).

### C1.1 backfill: EXISTS

- `backfill_content_identity` (`crates/db-app/src/content_identity.rs:152`), run at every startup after migrations and the legacy import, and after a claim. Idempotent.
  - Tables: `WORKSPACE_CONTENT_TABLES` (21: the 13 E2EE domain tables plus chats, entity mentions, folder attachments, document versions, proposals, voiceprints). Not included: CloudSync/e2ee bookkeeping, `shared_session_cache` (other people's notes, keyed by the sharer's workspace), `workspaces`/`workspace_memberships` (cloud projection), enterprise outbox. There are no `artifacts` or `chunks` tables; attachments are `session_attachments`.
  - `workspace_id = ''` gets the local workspace; `owner_user_id = ''` gets the local owner only on rows of the local workspace or `''`. Rows of any other (signed-in/team) workspace are never touched. The legacy `00000000-…` owner is left for the claim's rekey.
  - **No sync churn:** CloudSync syncs only `e2ee_records`; plaintext tables reach it through the `e2ee_dirty_rows` triggers. The backfill guards exactly its rows with `e2ee_apply_guard` (as sync's own apply does), so no dirty rows are queued. A `''` row was never uploaded (only workspaces with keys are), and it uploads with its next real edit. The search queue is refilled for changed sessions, which the new index fields need.
- The claim now also moves the non-E2EE content tables (`NON_E2EE_CONTENT_TABLES`, `binding.rs:199`), so no row is left in the pre-claim workspace.
- Tests (`content_identity.rs`): `identity_is_the_cloudsync_binding_and_library_owner`, `backfill_assigns_unassigned_rows_only_and_is_idempotent` (asserts zero dirty rows and no leftover guards), `backfill_refuses_an_empty_identity`, `claim_still_works_after_the_backfill_and_moves_every_content_table`.

### C1.2 / B4.1 search: EXISTS (search layer)

- Tantivy schema version 3 adds `workspace_id` and `session_id` (`plugins/tantivy/src/schema.rs:56`); the version change re-creates the index and the desktop indexer's projection version 5 (`apps/desktop/src-tauri/src/search_index.rs:13`) rebuilds it from SQLite. Rows with `workspace_id = ''` are indexed under `_unassigned` (`schema.rs:24`), which only a local scope matches.
- `search_collection` (`plugins/tantivy/src/ext.rs:455`) always adds a workspace clause from a `WorkspaceScope`, and a `session_id` clause when `filters.session_id` is set (an empty id matches nothing). `tantivy:search` takes the scope from the access state and returns an error with no workspace (`plugins/tantivy/src/commands.rs:21`).
- The indexer's count check uses `document_count` (`ext.rs:188`), which returns a number only.
- Tests: `search_returns_only_the_scoped_workspace` (C1.4 at the search layer), `session_filter_limits_results_to_one_meeting` (B4.2 at the search layer), `ext.rs:786`, `:830`.
- Ask: "This meeting" scope does not query Tantivy today (it reads the pinned meeting and pins `search_meeting_content`, `apps/desktop/src/chat/guardrails/tools.ts`). The filter is there for when it does. Ask's library search (`chat/guardrails/deps.ts`) goes through `tantivy:search`, so it is workspace-scoped.
- **Gap:** `tantivy:add_document`/`update_document(s)`/`remove_document`/`reindex` are still `Pending` webview commands; a webview could add documents with any workspace id. It cannot read another workspace through them.

### C1.2 Rust read paths: PARTIAL

- `aspc::WorkspaceScope` (`crates/aspc/src/scope.rs`): built only from a non-empty id. The local scope also matches `''` (unassigned rows belong to the local workspace by the claim's rule); a scope for any other workspace never does. Resolved per call from the database (`anlg_agent_access::local_scope`, `crates/agent-access/src/lib.rs:50`; `anlg_db_app::local_workspace_scope`), because a claim changes the local workspace while the app runs. No workspace → error (fail closed).
- **db-app** (`crates/db-app/src/session_ops.rs`): `list_sessions` (via `ListSessions.scope`, `:85`), `list_folder_paths` (`:130`), `get_session` (`:152`), `get_session_note`, `list_session_documents`, `list_session_transcripts`, `list_session_action_items` (each filters the row and its session), `list_session_participants` (`:230`; the joined humans and organizations too), `list_recurring_sessions`, `get_session_proposal`, `list_session_proposals`, `update_session_proposal_status`. Helper `push_workspace_scope` (`:12`).
- **agent-access** (CLI, MCP, local-api, `db:*` meeting commands): every public read takes `&WorkspaceScope`: `list_meetings` (`lib.rs:332`), `list_folders`, `get_meeting`, `get_meeting_transcript`, `get_recurring_meeting_history`, `get_meeting_export` (`:614`; speaker names `load_speakers` `:673` and speaker context scoped too), `get_local_meeting_export` (attachments scoped), and the proposals API (`proposals.rs`; a new proposal never gets an empty workspace).
- **Callers:** `db:list_meetings`, `get_meeting`, `get_meeting_transcript`, `get_recurring_meeting_history` (`plugins/db/src/commands.rs`); local-api export, cloud snapshot, Drive markdown, re-export and webhook payloads through `scoped_meeting_export` (`plugins/local-api/src/commands.rs:685`, `dispatch.rs`); CLI `meetings`, `proposals`, `meetings delete` export, and every MCP tool and resource (`apps/cli/src/commands/*.rs`, `apps/cli/src/mcp.rs`); `transcription:render_session_transcript` (`plugins/transcription/src/session_transcript.rs:32`, a session gate in the same transaction as its reads); voiceprint participant matching.
- **Export:** `plugins/export` writes content the webview passes in and reads no rows; `transcription:export_to_vtt` likewise. The Rust exports that read rows are local-api's and the CLI's, listed above.
- **Ask/chat:** no Rust tool side; the Ask tools run in the webview and read through `db:*` SQL and `tantivy:search`.
- **Not scoped yet (gap):** Rust paths that read content to change it, by an id the caller passes: `plugins/template` (summary/title/corrections), `plugins/calendar/src/contacts/*`, `plugins/transcription` speaker attribution, batch transcripts and voice enrollment, `plugins/session`, most `crates/db-app/*_ops.rs` write helpers, `session-ingest`, the vault mirror. Locally all rows are in one workspace (see C1 decision), so these return no other workspace's rows today; they need a scope before a second workspace can exist on a device. `get_recording_import` reads job status by job id (no content).

### C1.1 new rows: EXISTS (with the backfill as a net)

- **Rust:** almost every create path already wrote the local ids (from the parent session, or the binding as `insert_session` does). Fixed: `insert_pending_session_proposal` (wrote no workspace; now the session's, else the local one, `crates/db-app/src/session_review_ops.rs`) and `insert_folder_attachment` (wrote `''` for a folder with no meetings; now the local workspace, `folder_catalog_ops.rs`). `agent-access` proposals never use an empty workspace. Shared SQL: `LOCAL_WORKSPACE_ID_SQL` (`crates/db-app/src/content_identity.rs:22`).
- **Drizzle:** `workspace_id` and `owner_user_id` on the 17 content tables Drizzle defines use `$defaultFn` with SQL that reads the binding (`LOCAL_WORKSPACE_ID_SQL`, `LOCAL_OWNER_USER_ID_SQL` in `packages/db/src/schema.ts`). Drizzle prefers `.default()` over `$defaultFn`, so those columns no longer carry the TS-side `.default("")`; the SQL default in `app.db` is unchanged.
- **Frontend raw SQL:** chat groups and messages wrote a literal `''` (`apps/desktop/src/chat/store/queries.ts`); meeting chat and screen records left the column out (`apps/desktop/src/stt/meeting-*-records.ts`). All three fixed. The rest already took the session's or the binding's workspace.
- `tags`, `session_tags`, `folders`, `daily_notes` already have insert triggers that stamp the binding workspace (`e2ee_workspace_stamp_*`), which confirms the identity choice above.
- Tests: `rust_create_paths_never_write_empty_ids` (`content_identity.rs:474`) runs the Rust create paths (session, note, owner human and participant, contact, organization, manual participant, tag, session tag, note and audio attachments, folder, folder attachment, proposal, batch and live transcripts) and asserts no content row has an empty `workspace_id` or `owner_user_id` and every row is in the local workspace. `apps/desktop/src/shared/content-identity.test.ts` checks every frontend raw-SQL insert into a content table names `workspace_id` and never writes `''`. `packages/db/src/index.test.ts` checks a Drizzle insert fills both ids from the local identity.
- **Owner caveat:** paths that take an owner from the frontend keep what it passes. Signed out that is the legacy `00000000-…` id, which is non-empty and which the claim rekeys; it is not the local owner id. The legacy import runs before the startup backfill, which then fills its `''` rows.

### C1 decision: raw webview SQL (spike result)

**Decision: the TEMP-view mechanism is not shipped. Option (a), the single-workspace guard, is shipped as an interim.** It fails closed; it is not workspace filtering.

**Spike: TEMP views that shadow content tables** (`crates/db-execute/tests/workspace_views_spike.rs`, all `#[ignore]`; run with `cargo test -p db-execute --test workspace_views_spike -- --ignored`). Result on 2026-11-16, linked SQLite 3.50.2:

| Case                                          | Result                                                                                                                                                                                                                                                                                                                        |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Unqualified SELECT, joins, subqueries, CTEs   | filtered by the view (pass)                                                                                                                                                                                                                                                                                                   |
| `main.sessions`                               | **returns the other workspace's rows**; denying it needs the authorizer to check the database and accessor arguments, which it ignores today                                                                                                                                                                                  |
| Writes through the view (INSTEAD OF triggers) | **cannot be built**: a TEMP trigger may not name `main.sessions` in INSERT/UPDATE/DELETE ("qualified table names are not allowed … within triggers"), and the unqualified name is the view itself                                                                                                                             |
| UPSERT (`INSERT … ON CONFLICT`)               | **rejected on a view** ("cannot UPSERT a view" / "cannot modify … because it is a view"). The frontend writes sessions, notes, chats and more this way (`session/queries/sessions.ts`, `chat/store/queries.ts`, `session/insights/past-notes.ts`, `stt/meeting-consent-store.ts`, …), so Drizzle and those writes would break |
| Shared pool                                   | **TEMP views leak**: webview SQL runs on the shared pool, so a view on a pool connection also filters every trusted Rust query that later uses that connection (sync, claim, backfill, indexer). Needs a dedicated webview pool with the CloudSync extension and change hooks                                                 |

Because writes and UPSERT cannot go through views and `main.` bypasses them, the mechanism cannot be shown to fail closed without rewriting the frontend's writes.

**Shipped interim: single-workspace guard** (`crates/db-core/src/webview_sql.rs:460`, `WorkspaceGuard`; turned on in `plugins/db/src/runtime.rs:178`):

- Webview SQL reaches content tables only while every content row belongs to the local workspace (the CloudSync binding) or is unassigned (`''`). If any content row of another workspace exists, every content table (everything except settings, keys, and SQLite's schema and `pragma_*` functions) is denied to the webview until those rows are gone. Settings stay readable, so the app still opens.
- Same snapshot: Rust opens each webview statement's transaction (`BEGIN`, or `BEGIN IMMEDIATE` for anything that is not a plain SELECT/EXPLAIN/PRAGMA/VALUES) and runs the census inside it (`crates/db-execute/src/lib.rs:161`, `query.rs:59`, `plugins/db/src/runtime.rs` `run_webview_transaction`). The webview cannot issue `BEGIN`/`COMMIT`/`ROLLBACK` (authorizer, `webview_sql.rs:240`).
- Reads census only the content tables the statement names (`referenced_scoped_tables`, `:108`) and allow only those; reaching any other content table (a view, a table without `workspace_id`) is denied and retried after a full census in the same transaction. Writes run the full census.
- Writes are confined: after a statement that changed rows, the full census runs again before commit, and a write that created another workspace's rows is rolled back (`check_writes`, `:568`).
- No binding, or a census error, denies content (fail closed). Live-query dependency analysis runs the same check (`crates/db-reactive/src/schema.rs`).
- Denials log `webview_sql_denied_multiple_workspaces` with the table names (no rows).
- Tests (`crates/db-execute/tests/workspace_guard.rs`):
  - `a_second_workspace_denies_every_content_path`: unqualified SELECT, `main.sessions`, joins, scalar and EXISTS subqueries, CTEs (also recursive), a view, a table without `workspace_id`, `EXPLAIN QUERY PLAN`, UPDATE/DELETE without WHERE, `RETURNING` on INSERT/UPDATE/DELETE, INSERT … SELECT, `execute_proxy`, `execute_transaction`; `sqlite_master`, `pragma_table_info`, `PRAGMA table_info` and `app_settings` stay readable; nothing was changed; access returns once the rows are gone.
  - `webview_writes_cannot_create_another_workspace`: INSERT with a foreign `workspace_id` (with and without `RETURNING`), `UPDATE … SET workspace_id`, a write CTE, and a transaction whose second statement adds a foreign row (its first statement is rolled back too).
  - `webview_cannot_control_transactions`, `no_binding_fails_closed`, `census_failure_fails_closed`, `census_and_statement_share_one_snapshot` (a foreign row committed after the census is invisible to the statement), `guard_is_off_unless_enabled`, `single_workspace_reads_and_writes_work` (UPSERT, RETURNING-free writes, transactions, a view, `EXPLAIN QUERY PLAN`).
  - Reactive layer: `workspace_guard_keeps_live_queries_working_and_fails_closed` (`crates/db-reactive/src/runtime.rs:623`): a live query and its table-dependency refresh work with one workspace; when another workspace's row appears the refresh is an error event, never rows, and a new subscription gets an error.
  - `webview_guard_census_covers_every_content_table` keeps the census list equal to `WORKSPACE_CONTENT_TABLES`.
- **Cost:** on a synthetic large library (493 MB: 3,000 meetings, 40,000 note versions) the full census takes about 27 ms, a census of `sessions` 0.1 ms, `transcripts` 1.5 ms, `session_documents` 2.8 ms. Reads pay only for the tables they name; writes pay the full census while holding the write lock.
- **Risk to check by hand:** a device whose database already holds another workspace's content rows is locked out of all content in the UI. One known source: legacy folders assigned to a team workspace (`folders.workspace_id`, see `legacyTeamWorkspaceId` in `apps/desktop/src/folders/folder-editor.tsx`). Detect with the SQL in "Phase 2b runtime check".
- **What it is not:** workspace filtering. The view approach, or moving webview reads to scoped Rust queries table by table, is still needed before a device may hold two workspaces' content. Options for that:
  - (b) move webview content reads and writes to scoped Rust commands, table by table (largest change, cleanest end state);
  - (c) views on a dedicated webview connection pool, plus an authorizer that denies `main.`/`temp.`-qualified access by database and accessor, with frontend writes moved off UPSERT (or to Rust commands) because views reject UPSERT and the linked SQLite rejects qualified names in TEMP triggers.

### Phase 2b runtime check

Nothing in Phase 2b was run in the app. Check by hand:

1. **Startup backfill:** quit the app, run `sqlite3 "<app data>/app.db" "SELECT 'sessions', count(*) FROM sessions WHERE workspace_id='' OR owner_user_id='' UNION ALL SELECT 'transcripts', count(*) FROM transcripts WHERE workspace_id=''"`, start the app, check `app.log` for `aspc_content_identity_backfilled`, and run the query again: zero.
2. **No sync churn:** signed in with sync on, compare `SELECT count(*) FROM e2ee_dirty_rows` before and after the first launch: unchanged by the backfill.
3. **Locked-out check:** before launching, `SELECT DISTINCT workspace_id FROM folders UNION SELECT DISTINCT workspace_id FROM sessions UNION SELECT DISTINCT workspace_id FROM humans` and compare with `SELECT value_json FROM app_settings WHERE id='cloudsync_workspace_binding'`. Any other non-empty id will lock the webview out of content; `app.log` then shows `webview_sql_denied_multiple_workspaces` with the tables.
4. **Everyday use:** notes list, open a note, edit and rename, chat (new thread, messages), search, record a short meeting, enhance; all should work and nothing should log `aspc.wrong_workspace`.
5. **Search:** after the first launch the index rebuilds (schema 3). Search finds notes and contacts; "This meeting" Ask still answers from the pinned meeting.
6. **Sign-in:** sign in to CloudSync on a device with local notes. The claim must succeed (no `ForeignWorkspace`), notes stay visible, and `access_context` reports the account id as workspace afterwards (`local-auth:access_context`).
7. **CLI/MCP:** `anarlog meetings list`, `meetings show`, `meetings export` and an MCP `list_meetings` call return the same notes as the app.
8. **Speed:** with a large library, scrolling the notes list and typing in a note should not feel slower. Writes now run the full census in their transaction.

## Phase 3 changes (Efficiency Engine and prompt safety), 2026-11-16

Nothing below was run in the app; see "Phase 3 runtime check".

### C7 Efficiency Engine: EXISTS

- **Core** `crates/ai-engine` (no Tauri):
  - config: `HAIKU_MODEL` (`src/config.rs:13`), `ANTHROPIC_API_VERSION` (`:17`), `DIRECT_ENDPOINT` (`:20`), `resolve_engine_config` (`:101`), the only place mode, endpoint and auth are decided;
  - request rewrite: `ALLOWED_PATHS` (`src/request.rs:13`), `MAX_OUTPUT_TOKENS` (`:17`), `ALLOWED_BETAS` (`:24`), `prepare` (`:65`) overwrites `model`, strips sampling and side-service fields;
  - upstreams: `DirectUpstream` (`src/transport.rs:19`, no redirects, key in the configured header), `ProxyUpstream` stub (`:83`, `:94`) with the implementation notes;
  - usage and quotas: defaults (`src/usage.rs:53`), ranges (`:47`), `validate` (`:66`), `check` (`:136`), `RateLimiter` (`:218`); files `<app data>/ai-engine/usage.json` and `quotas.json` (counts only, no schema change);
  - `Engine::request` (`src/lib.rs:217`) streams the response and records tokens from the SSE `message_start`/`message_delta` usage; `map_status` (`:404`) maps 429/401/403/5xx to the pause codes; `redact` (`src/redact.rs:24`) cleans provider error messages (C3.4).
- **Commands** `plugins/ai-engine/src/commands.rs`, in the registry (`crates/aspc/src/commands.rs:124-131`), all guarded except `cancel_request` (`Public`, flow control):
  - `request` `(Notes, Read)` (`:29`), so the composer can use it;
  - `status` `(Settings, Read)` (`:57`): mode, model, usage, quotas, last pause;
  - `key_status` `(Keys, Read)` (`:69`, presence only), `set_api_key` / `clear_api_key` `(Keys, Edit)` (`:80`, `:93`, full grant required);
  - `set_quotas` `(Settings, Administer)` (`:106`).
  - Capabilities: `main.json` gets `ai-engine:default`; `composer.json` gets only `allow-request`, `allow-cancel-request`, `allow-status`.
- **Frontend** `apps/desktop/src/ai/engine/`: `createEngineModel` (`model.ts:25`), `createEngineFetch` (`transport.ts:68`), `AiEngineError` and the "AI paused: …" message (`errors.ts`), status hooks (`status.ts`). `useLanguageModel` returns only this model (`ai/hooks/useLLMConnection.ts:49`). Settings → AI (`settings/ai/engine/index.tsx:20`): mode, model, key present or missing, a write-only key field, today's and this month's usage against the quotas, editable limits, the pause reason.
- **Tests:** `crates/ai-engine/src/tests.rs` (stub HTTP server, fake key: pinned model, forbidden paths, `ai.no_key`, 5xx/429/401/400 mapping without key leaks, network error, quotas and rate limit, PROXY stub, malformed keys, cancel), `src/request.rs`, `src/usage.rs`, `src/stream.rs`, `src/redact.rs` unit tests; `tests/no_ai_call_sites.rs` (C7.1); `apps/desktop/src/ai/engine/engine.test.ts`; `store/zustand/ai-task/task-configs/enhance-one-call.test.ts` (C7.5).
- **Removed** (only reachable through the old picker): `settings/ai/llm/` (provider and model picker, configure, health check, subscription OAuth for Claude/ChatGPT/Grok/Copilot, usage line), `settings/ai/shared/list-{anthropic,openai,google,mistral,ollama,lmstudio,openrouter,venice,unsloth,azure-ai,azure-openai,cloudflare-workers-ai,apple-foundation}.ts`, `local-provider-availability.ts`, `ai/{apple-foundation-model,reasoning-effort,stream-only-generation,auth-fetch,traced-fetch}.ts`, the LLM half of `shared/config/configure-paid-settings.ts`; the desktop dependencies `@ai-sdk/{azure,google,openai,openai-compatible}`, `@openrouter/ai-sdk-provider`, `ollama`, `@anlg/plugin-local-llm`; the `bedrock` and `local-llm` plugins are no longer registered in the app (`apps/desktop/src-tauri/src/lib.rs`, `Cargo.toml`, capabilities). Their sources stay in `plugins/` (and in the registry, which lists every plugin directory).
- **Rust-side AI in the desktop dependency tree:** none besides the Engine. `plugins/agent` / `crates/claude` only run the local `claude` CLI for a version and auth check and edit its settings file (no model call); `crates/summary` and `plugins/template` are pure prompt helpers; `plugins/bedrock` (model listing) and `plugins/local-llm` (on-device and local-server models) are unregistered. `crates/owhisper-client` (speech-to-text) is out of scope. The hosted `crates/llm-proxy` is not a desktop dependency and is unchanged.
- **Gaps:** PROXY mode is a stub (gate C7.6); spend caps are **[hosted]**; the old `ai_usage_daily_limit` Ask limit only applied to the hosted `anarlog` provider and is now inert (the Engine quotas replace it); the title workflow (`title-workflow.ts`) is no longer started by anything.

### C7.5 one call per meeting: EXISTS

- The enhance system prompt asks for a `<meeting_title>` line first (`crates/template-app/assets/enhance.system.md.jinja:33`). `withTitleLine` cuts it from the stream (`enhance-workflow.ts:183`, `enhance-title.ts`), and `runEnhanceSuccess` saves it with the summary (`enhance-success.ts:71`); no title task is started. SDK and validation retries are off for enhance (`enhance-workflow.ts:27-28`).

### B3.1 escaping: EXISTS

- `escape_block_content` (`crates/template-app/src/escape.rs:32`, tags at `:15`) runs over every external string of `EnhanceUser` and `TitleUser` before rendering (`crates/template-app/src/lib.rs:81,85`) and over every `render_custom` context value (`plugins/template/src/ext.rs:17`). Tests in `escape.rs`: hostile transcript and note keep one of each block tag; every tag in `assets/` is listed.

### B3.3 unscoped chat: EXISTS

- A request without `askScope` gets `selectToolsForScope(…, { scope: "library", explicitEdit: false })` (library read tools, no write, no web) and `UNSCOPED_DATA_RULE` (`apps/desktop/src/chat/transport/index.ts:76,626`). The Automations chat keeps working for questions; it can no longer edit, move or search the web.

### B3.4 / B4.2 injection tests: EXISTS

- Enhance: `enhance-one-call.test.ts` ("B3.4" case). Ask: `chat/guardrails/injection.test.ts` runs a stub model that calls `get_meeting`/`search_meeting_content` for meeting B, `edit_summary`, `create_folder`, `web_search` and invented export/delete/key tools, through the real transport, tool loop and Engine client. Result: no write, web or key read; B's marker never appears in any request or the answer; unscoped requests expose no write or web tools.

### C3.2 webview key reads: PARTIAL

- `store2:get_secret` refuses `ai-provider-api-keys:llm:*` to the webview (`plugins/store2/src/commands.rs:31,47,363`; test `webview_cannot_read_llm_keys_but_keeps_stt_and_import_secrets`). Still readable by the webview, because it passes them to Rust commands: STT keys (`ai-provider-api-keys:stt:*`, `stt/useSTTConnection.ts`, dictation) and connected-import tokens (`meeting-imports:*`, `imports/connected-import.ts:343`).
- The stale Phase 2a test `renderer_secret_commands_reject_native_accounts_before_keychain_access` (it called the guarded commands without a window and no longer compiled) now tests the renderer path behind the guard.

### A3.3 HTTP scope: narrowed

- Removed from `main.json` and `composer.json`: `api.anthropic.com`, `claude.ai`, `platform.claude.com`, `auth.openai.com`, `auth.x.ai`, `chatgpt.com`, `api.githubcopilot.com`, `api.cerebras.ai`, `api.cohere.ai`, `api.deepseek.com`, `api.fireworks.ai`, `api.kimi.com`, `api.moonshot.ai`, `api.venice.ai`, `*.openai.azure.com`, `*.services.ai.azure.com`, `aiplatform.googleapis.com`, `*-aiplatform.googleapis.com`, `gateway.ai.cloudflare.com`. Kept: hosts the speech-to-text providers use (including `api.openai.com`, `api.groq.com`, `openrouter.ai`, `generativelanguage.googleapis.com`, Bedrock, Azure Speech, Cloudflare), GitHub, the app API, Supabase and loopback.

### C7.7 secrets check: EXISTS

- `scripts/check-secrets` (`check-secrets.mjs`) scans tracked Rust and desktop sources and the built bundle (`apps/desktop/dist`, `--bundle`, `--require-bundle`) for Anthropic, OpenAI-style, AWS and Google API keys; fake keys pass only in test files. `scripts/check-secrets.test.mjs` runs in `ci.yaml`. Not wired into the release workflow yet: run `scripts/check-secrets --require-bundle` on the release build (C7.6 checklist).

### Phase 3 runtime check

1. **Settings → AI:** shows Mode Direct, Model `claude-haiku-5-5`, Key missing. Enter a key: the field clears, Key shows "saved in Keychain". "Remove" clears it. A key not starting with `sk-ant-` is refused with a message.
2. **Enhance one meeting** with a key: the summary streams without a `<meeting_title>` line; an untitled meeting gets a title. Settings → AI "Today" goes up by one call.
3. **Ask** in This meeting and All meetings; **the composer** (quick Ask): answers stream; no `aspc.window_denied` in `app.log`.
4. **AI down:** with no key, enhance and Ask show "AI paused: no API key is set…"; set daily calls to 1 and enhance twice: the second says the limit is reached. Offline: "AI paused: the AI service can't be reached…". Recording, transcripts and notes keep working in each case.
5. **Speech-to-text** with a BYOK provider still works (its key is still readable; its host is still in the scope).
6. **Automations chat** answers questions but makes no edits.
7. Old LLM settings rows (`current_llm_provider`, provider keys under `llm:*`) are ignored; nothing errors on boot.

## Phase 4 changes (audit log, Private, permanent delete, retention), 2026-11-16

Nothing below was run in the app; see "Phase 4 runtime check". Approved schema changes only: the separate `audit.db` and `sessions.private`.

### D1 audit log: EXISTS

- **Core** `crates/audit-log` (no Tauri unless the `tauri` feature):
  - `open` (`src/lib.rs:195`) returns an append-only `AuditWriter` and an `AuditReader`; `open_append_only` (`:209`) gives the CLI a writer only, without the Keychain;
  - table and append-only triggers `SCHEMA` (`:84`), file mode `0600`; `append` (`:281`) runs in `BEGIN IMMEDIATE`, so the app and the CLI never fork the chain;
  - entry validation (`src/entry.rs:107`, `is_identifier` `:154`): ids and counts only, no whitespace (D1.5); canonical form `:186`;
  - Keychain head (`src/head.rs`, item `audit-chain-head` `:19`, worker thread `:101`): reads and writes off the caller's thread with a 3 s timeout; `reconcile_head` (`src/lib.rs:449`) stores the head on first run, freezes it on a mismatch; `check_head` (`:352`);
  - `verify` (`:505`): walks the chain (missing seq, broken link, hash mismatch), then checks the end against the Keychain head (truncation, a consistently rewritten chain); `list` (`:481`) pages newest first;
  - denial rate limit (`src/limiter.rs:46`), used by `submit_denial` (`src/lib.rs:420`); Tauri helpers `record`, `record_debounced`, `record_transcript_access`, `denial_entry` (`src/tauri_helpers.rs:85`).
- **Commands** `plugins/audit` (registry `crates/aspc/src/commands.rs:135-139`): `list_entries` and `verify` `(Settings, Administer)`; `record_meeting_view` and `record_ask` `(Notes, Read)`; `record_settings_change` `(Settings, Edit)`. All guarded. The reader lives in a state type private to the plugin (`plugins/audit/src/lib.rs:16`). Capabilities: `main.json` gets `audit:default`, `composer.json` only `audit:allow-record-ask`.
- **Wiring** `apps/desktop/src-tauri/src/audit.rs`: opens `audit.db` next to `app.db` (`lib.rs:197`, a failure only logs), registers the plugin (`lib.rs:353`) and `wire` (`audit.rs:26`, called at `lib.rs:442`): guard denials through `AspcState::on_denial` (`crates/aspc/src/state.rs:109`, fired at `:211`), webview SQL denials through `WebviewSqlAuthorizer::set_denial_sink` (`crates/db-core/src/webview_sql.rs:259`), the `db` plugin's transcript reads through `set_content_access_observer` (`plugins/db/src/runtime.rs:667`, so the `db` plugin itself has no audit handle), and the Keychain reconcile in the background.
- **Events** (D1.4):
  - `app.unlocked`: `plugins/local-auth/src/app_lock.rs:97`;
  - `meeting.viewed`: `apps/desktop/src/session/index.tsx:85` → `audit:record_meeting_view` (`plugins/audit/src/commands.rs:56`), debounced 5 min per meeting in Rust;
  - `meeting.exported`: PDF `plugins/export/src/commands.rs:32`, VTT `plugins/transcription/src/listener2/commands.rs:95`, Markdown `plugins/local-api/src/commands.rs:240`, CLI `apps/cli/src/lib.rs:107`;
  - `transcript.accessed`: `render_session_transcript` (`plugins/transcription/src/listener2/commands.rs:195`), `db:get_meeting_transcript` (`plugins/db/src/commands.rs:127`), debounced;
  - `ask.query`: `apps/desktop/src/chat/transport/index.ts:488` → `audit:record_ask` (`commands.rs:79`): scope, meeting ids, count, never the text;
  - `settings.changed`: `settings:save` with changed key names (`plugins/settings/src/commands.rs:78`), app settings writes (`apps/desktop/src/settings/queries.ts:355` → `record_settings_change`), the lock (`app_lock.rs:132`), Engine quotas (`plugins/ai-engine/src/commands.rs:141`);
  - `key.changed`: Engine key set/clear (`plugins/ai-engine/src/commands.rs:92`), `store2` secrets by name (`plugins/store2/src/commands.rs:449`);
  - `permission.denied`: guard and SQL authorizer, rate-limited;
  - `meeting.deleted`: CLI hard delete (`apps/cli/src/lib.rs:61`); the app's permanent delete and retention are below.
- **UI** Settings → Audit (`apps/desktop/src/settings/audit/index.tsx:17`): paged entries and Verify with the result and the head note.
- **Tests:** `crates/audit-log/src/tests.rs` (chain and paging, triggers refuse UPDATE/DELETE, edited / deleted / reordered entries point to the first bad seq, truncation and a recomputed chain are caught by the Keychain head even after reopening, first run stores the head, a hanging Keychain blocks neither appends nor verify, a denial loop writes one entry, content is refused, the CLI's append-only handle), `src/entry.rs`, `src/limiter.rs`; `tests/isolation.rs`; `webview_sql_cannot_reach_the_audit_log` and `denials_reach_the_denial_sink` (`crates/db-core/src/webview_sql.rs`); `denials_reach_the_denial_listeners_with_the_window` (`crates/aspc/src/state.rs`); `changed_keys_names_only_what_changed` (`plugins/settings`).
- **Gaps:**
  - most settings are written by the webview with raw SQL; their key names reach the log only because the webview reports them (`record_settings_change`). A hostile webview can skip that report. The `store2` plain values (`set_str`/`set_bool`/`set_number`, UI state) are not logged.
  - webview raw SQL reads of transcripts (the transcript view) are not logged one by one; "meeting viewed" covers opening the meeting.
  - MCP tool calls through the CLI are not logged (only CLI delete and local export are).
  - a head mismatch is never cleared: after a real incident Verify keeps failing until the Keychain item is removed by hand.
  - `meeting.shared` exists as an action but sharing is a webview flow; it is not recorded yet.

### C6.4 Private: EXISTS

- **Schema** (approved): `crates/db-app/migrations/20261116120000_session_private.sql` adds `sessions.private INTEGER NOT NULL DEFAULT 0` (downgrade-safe), step `crates/db-app/src/lib.rs:625` with scope `CloudsyncAlter { sessions }` like `20260820120000_session_locked`. Drizzle `packages/db/src/schema.ts:237`; `SessionRow.private` and `SESSION_COLUMNS` (`crates/db-app/src/session_types.rs`, `session_ops.rs`). Sync: the E2EE replica seals every `sessions` column read with `PRAGMA table_info` (`crates/db-app/src/e2ee/replica_storage.rs:684`), so `private` flows exactly like `locked`, with no projection list to edit; test `private_flag_reaches_other_devices` (`crates/db-app/src/e2ee/tests/roundtrip.rs:340`). No mobile-only schema file exists (mobile runs `anlg_db_app::schema()`).
- **UI:** "Make Private" / "Make Not Private" in the meeting's overflow menu and a "Private" indicator in the header (`apps/desktop/src/session/components/outer-header/overflow/private-note.tsx:11`, `:36`). Written through `updateSession` like `locked`.
- **All-meetings Ask:**
  - `tantivy:search` fills `exclude_session_ids` in Rust from `PrivateSessionSource` (`plugins/tantivy/src/commands.rs:32`, type `src/lib.rs:104`, installed by `apps/desktop/src-tauri/src/search_index.rs:41`) unless the search has `session_id` or `include_private`; the filter is a `MustNot` term (`plugins/tantivy/src/ext.rs:619`). Without the source an all-meetings search fails closed. The field is `#[serde(skip)]`, so the webview can't set it.
  - `db:list_meetings` / recurring history: `list_sessions` leaves them out unless `include_private` (`crates/db-app/src/session_ops.rs:94`).
  - library tools: `excludePrivateMeetings` (`apps/desktop/src/chat/guardrails/private.ts:73`, applied in `tools.ts:168`) refuses a Private id and strips Private meetings from results; meeting titles `deps.ts:62`. "This meeting" tools are pinned and unwrapped.
- **Bulk exports:** `agent_access::list_meetings` excludes, `list_meetings_including_private` includes (`crates/agent-access/src/lib.rs:334`, `:345`); MCP and the local API's cloud snapshot ids use the excluding one; CLI `meetings list --include-private` (`apps/cli/src/cli.rs:253`, documented in `docs/reference/cli.mdx`). Markdown export automation (`plugins/local-api/src/commands.rs:162`) and webhook dispatch (`plugins/local-api/src/dispatch.rs:215`, helper `:192`, fails closed) skip Private meetings. Single-meeting exports (PDF, Markdown, CLI `export ID`) are explicit and unchanged.
- **Sharing:** new share of a Private meeting asks for confirmation (`apps/desktop/src/session-sharing/index.tsx:218`, `assertShareable` `source.ts:83`); folder default access skips it (`folder-default-access-runner.tsx:216`); folder sharing payloads leave it out (`resource-sharing/payloads.ts:106`).
- **Tests:** `list_sessions_excludes_private_meetings_unless_included` (`session_ops.rs:566`), `private_meetings_are_left_out_of_listings` (`agent-access/src/lib.rs:1134`), `excluded_sessions_drop_out_unless_scoped_to_them` (`plugins/tantivy/src/ext.rs:802`), `private_meetings_are_not_dispatched_or_auto_exported` (`plugins/local-api/src/dispatch.rs:575`), `private_flag_reaches_other_devices`, `apps/desktop/src/chat/guardrails/private.test.ts`, `session-sharing/source.test.ts` ("marks Private meetings…").
- **Gaps:**
  - marking a meeting Private does not revoke an existing share, and the share panel can still publish updates to it; revoke it there;
  - there is no "Include private meetings" switch in the Ask UI (the `includePrivate` option exists in `selectToolsForScope` and `tantivy:search` but nothing turns it on), so all-meetings Ask always leaves them out;
  - the flag is written by webview SQL, so a hostile webview could clear it; the Rust exclusions read the database each time, so they follow whatever the flag says;
  - Private changes are not audit-logged.

### D2.2 permanent delete: EXISTS

- **Before:** delete was a soft tombstone with a 5 s undo; after it only the session folder was removed (`finalizeSessionDeletion`); rows, versions, live deltas, chats and search documents stayed forever. Only the CLI had a hard delete, for unsynced meetings.
- **Database** `crates/db-app/src/session_purge.rs`:
  - `purge_deleted_session` / `_with` (`:257`, `:267`): only for a soft-deleted meeting; unsynced → `delete_local_rows` (`:47`) plus the `sessions` row; synced (`session_is_synced` `:218`) → `blank_rows` (`:309`) on `sessions` and `SYNCED_SESSION_TABLES` (`:187`), keeping only `TOMBSTONE_COLUMNS` (`:201`), under the E2EE apply guard (so restore-on-edit and content-version triggers don't fire) and queued in `e2ee_dirty_rows` the way the sync apply path does; then every other row is deleted. Idempotent: a second run changes and queues nothing.
  - `delete_local_rows`: app settings capture entries, transcript live state (deltas cascade), voiceprint exemplars, "This meeting" Ask chats (`chat_messages` with `askMeetingId`, and their group when nothing else is in it), every table with a `session_id` column (enumerated from SQLite once per run, `PurgePlan`), entity mentions, `auto_enhance_pending`. The CLI's `purge_local_session` now shares it.
  - `soft_deleted_session_ids` (`:405`) for the sweep.
- **Orchestration** `plugins/session/src/purge.rs`: `purge_sessions_in_db` (`:53`, `secure_delete` on, one `BEGIN IMMEDIATE` per meeting, `checkpoint` `:119` TRUNCATE with retries), `purge_session_files` (`:144`, vault folders named after the meeting, cache entries containing its id, symlinks not followed), `purge_sessions` (`:243`, also Tantivy and the audit entries), `purge_expired_soft_deletes` (`:329`). Command `session:purge_deleted_session` (`plugins/session/src/commands.rs:81`, registry `crates/aspc/src/commands.rs:542`, `(Notes, Delete)`, guarded).
- **Search index** `plugins/tantivy/src/ext.rs:413`, `:429`: deletes by `session_id` and `id`, merges all segments, garbage-collects files.
- **Database file** `crates/db-core/src/lib.rs:348`: every local `Db::open` connection runs with `secure_delete = ON`.
- **Webview** `apps/desktop/src/session/queries/deletion.ts:159`, called from the undo toast's finalize after the share is revoked (`useDeleteSession.ts:202`, and the forwarded-deletion and fallback paths).
- **Quit during the undo window:** the meeting is deleted on the next launch (`apps/desktop/src-tauri/src/retention.rs:8`, waits for the index and the database, then `purge_expired_soft_deletes`). Existing tombstones from before this build are purged by the same sweep.
- **Sync:** the E2EE replica uploads the blanked rows (all fields of a dirty row are resealed), so the server keeps ciphertext of blank values; receiving devices apply `deleted_at` and the blank values, and their own sweep purges local-only rows and files. `deletion_context` is cleared because its observations can contain an attachment name and because a cleared context means no device restores the meeting on an older edit.
- **Tests:** `a_purged_meeting_leaves_no_marker_in_the_database_wal_vault_or_cache` (`plugins/session/src/purge/tests.rs:106`: marker in title, note (edited), meeting chat, transcript, live delta, attachment name, Ask chat, audio, recovery chunk, attachment file, waveform cache; byte search of `app.db`, `app.db-wal`, the vault and the cache), `purged_sessions_leave_no_bytes_in_the_index_directory` (`plugins/tantivy/src/ext.rs:840`), `an_unsynced_meeting_leaves_no_row_behind` (`session_purge.rs:535`), `a_synced_meeting_keeps_only_a_blank_tombstone_and_queues_it` (`:573`), `useDeleteSession.test.tsx` (purge only after the undo window and after the share revoke).
- **Gaps:**
  - the marker test covers the database, WAL, vault and cache in one test and `search_index/` in a second one (the index code lives in the Tantivy plugin); there is no single end-to-end test through the app;
  - content freed before this build (old free pages) can stay in `app.db` until a `VACUUM`; the app doesn't vacuum (the CLI delete does);
  - local `e2ee_records` / `e2ee_local_state` rows of a synced meeting keep ciphertext and value tags until sync replaces them with the blank values; nothing deletes them directly;
  - cloud attachment blobs: the tombstone keeps `cloud_object_key`; whether attachment sync removes the blob for a tombstoned row was not checked (GUESS: it does through its delete guard);
  - if readers hold the WAL for longer than ~1 s, the checkpoint doesn't truncate and old pages stay in `-wal` until the next checkpoint;
  - global chats (not "This meeting") that quoted the meeting are not deleted.

### D3 retention: EXISTS

- **Setting:** `meeting_retention_days` (`apps/desktop/src/settings/schema.ts:261`, number, default 0 = forever); Settings → Privacy `RetentionSetting` (`apps/desktop/src/settings/privacy/retention.tsx:28`): "Keep meetings forever" / "Delete meetings older than…" with a day count. Settings-store key only, no schema change. Writing it is reported as `settings.changed` (key name).
- **Core** `plugins/session/src/retention.rs`: `retention_days` (`:30`, `parse_days` `:39` accepts 1–36 500), `expired_session_ids` (`:50`, local workspace, `julianday` of start or creation time, skip set), `soft_delete_for_retention` (`:77`), `apply_retention` (`:103`): soft delete under the synced-write guard, `purge::purge_sessions` (D2.2), `retention.run` audit entry (period, deleted, purged, skipped recording).
- **Job** `apps/desktop/src-tauri/src/retention.rs`: `spawn` (`:16`) waits for the index and database, then every `RETENTION_INTERVAL` (6 h, first tick at startup) `run_once` (`:31`): the D2.2 sweep, then retention with the meetings being recorded or finalized from the capture snapshot (`recording_sessions` `:63`; unknown state → skip the run).
- **Tests:** `old_meetings_are_deleted_and_newer_or_recording_ones_kept` (`plugins/session/src/retention.rs:176`, fixed clock, includes a timezone offset, an unparseable date, a meeting 1 s inside the period that expires a day later, and a recording meeting), `keep_forever_deletes_nothing` (`:271`), `retention_setting_parses_to_days_or_forever`.
- **Before:** only an audio-only sweep existed (`plugins/transcription/src/audio_retention.rs`); it is unchanged and separate.
- **Gaps:** a changed setting applies at the next run (startup or within 6 hours), not immediately; with sync on, every device with the setting applies it to the shared library (the tombstones then delete it everywhere), so a short period on one Mac deletes meetings on all of them.

### Phase 4 test results (2026-11-16, this Mac)

- Rust: `audit-log` 15 unit + 2 isolation; `aspc` 24; `db-core --lib` 107 (the `e2e` test needs `SQLITECLOUD_URL` and fails without it, as before); `db-app` 285 before the purge tests, plus `session_purge` 2 and `private_flag_reaches_other_devices`; `agent-access` private test; `tauri-plugin-tantivy` 14; `tauri-plugin-local-api` private test; `tauri-plugin-session` 43; `tauri-plugin-settings` audit test; `tauri-plugin-audit` 2; `anarlog-cli` 44 (CLI contract snapshot updated for `--include-private`). All pass. `cargo clippy --all-targets --no-deps -- -D warnings` is clean for `audit-log`, `db-app`, `db-core`, `agent-access`, `anarlog-cli`. `cargo check -p desktop` passes.
- Desktop: `tsc --noEmit` passes; `oxlint` 0 errors; vitest in batches covering every folder: all pass except two `settings/general/billing.test.tsx` cases that also fail on the commit before Phase 4 (date-dependent). Some component tests took minutes on this Mac because cold reads of `@hugeicons/core-free-icons` (12 000 files) are slow; they pass once the files are cached.

### Phase 4 runtime check

1. **Audit log file:** after a launch, `audit.db` sits next to `app.db` (mode `0600`). Keychain Access shows an item `<bundle id>.audit-log` / `audit-chain-head` after the first entry. A Keychain prompt, if any, does not hold up startup.
2. **Settings → Audit:** unlock the app, open a meeting, export it as PDF, run an Ask query in each scope, change a setting and the Engine key: each shows one entry with ids only (no title, no query text, no key value). Opening the same meeting again within 5 minutes adds nothing.
3. **Verify:** says OK with the entry count. Close the app, then in a copy of the data folder run `sqlite3 audit.db "DROP TRIGGER audit_entries_no_update; UPDATE audit_entries SET object_id='x' WHERE seq=2"`; on the next launch Verify points to entry 2. (Restore the original file afterwards.)
4. **Denials:** lock the app with a note window open; Settings → Audit later shows a few `permission.denied` entries with a `suppressed` count, not hundreds.
5. **Keychain unavailable:** with the login keychain locked, the app still starts and Verify says the head could not be read.
6. **Delete:** delete a meeting, wait 6 seconds: its folder under the vault's `sessions/` is gone, the search finds nothing from it, and Settings → Audit shows `meeting.deleted` and `data.deleted`. Delete one and quit within 5 seconds: on the next launch it is gone (not restored). With sync on, a second Mac drops the meeting after its next sync and its own sweep.
7. **Retention:** Settings → Privacy → "Delete meetings older than…" 1 day, relaunch: meetings older than a day are gone (not one being recorded), and Settings → Audit shows `retention.run` with the count. Set it back to "Keep meetings forever".
8. **Private:** mark a meeting Private from its "…" menu; the header shows "Private". All-meetings Ask about a phrase only in that meeting finds nothing; "This meeting" Ask on it still answers. `anarlog meetings list` leaves it out, `--include-private` shows it. Sharing it asks first; declining shows "Not shared: this meeting is Private." A second Mac with sync shows it as Private too.

## Phase 5 changes (defaults, encryption check, backup, privacy surfaces, licences), 2026-11-16/17

Nothing below was run in the app; see "Phase 5 runtime check".

### D1 denial entries for retried reads: FIXED (Phase 4 bug)

- **Bug:** the webview SQL authorizer reported every denial to the audit sink, including the provisional ones of a read's first attempt under the C1 guard's partial census (only the tables the SQL names are allowed), which the caller then retries after a full census and which succeed. The app logged `permission.denied` / `sql.workspace` for `events`, `json_each`, `local_recording_import_jobs` at startup with no real denial.
- **Fix** `crates/db-core/src/webview_sql.rs`: `WorkspaceGuard::check_read` starts holding denials in `ConnectionState.deferred_denials`; `WorkspaceGuard::check` (the full census before the retry) drops them, so the retry reports its own; `AuthorizedConnection::release` and its `Drop` report any still held (a first attempt that was never retried). The allow/deny decisions are unchanged (fail closed as before). Both retry sites are covered: `crates/db-execute/src/lib.rs` (`guarded`) and `crates/db-reactive/src/schema.rs` (`resolve_query`).
- **Test:** `only_final_denials_reach_the_denial_sink` (`crates/db-execute/tests/workspace_guard.rs`): a view read, a read of a table without `workspace_id` and a `json_each` join report nothing in single-workspace mode; with a second workspace present the denials reach the sink. It fails on the Phase 4 code (three spurious denials).

### C4.3 / C4.4 / D6 analytics and session replay defaults: EXISTS (was MISSING: analytics on by default, replay with telemetry)

- **Before:** `telemetry_consent` defaulted to true (UI), PostHog's Rust store treated a missing `Disabled` key as enabled, Session Replay ran (10% / 100% on error) whenever telemetry and crash consent were both on, and the UI showed crash reporting on by default while Rust treated it as off.
- **Now:**
  - `apps/desktop/src/settings/schema.ts`: `analytics_opt_in` and `session_replay_opt_in` (default false); `telemetry_consent` kept only for the crash-reporting split migration, default false; `crash_reporting_consent` default false (matches `crash_reporting_consent_from_rows`, `apps/desktop/src-tauri/src/lib.rs:75`).
  - `apps/desktop/src/settings/queries.ts` (`applySettingSideEffects`): `analytics_opt_in` drives `analytics:set_disabled`; `session_replay_opt_in` drives replay. The legacy key no longer touches either.
  - `plugins/analytics/src/ext.rs` (`set_disabled`, `is_disabled`, `analytics_disabled`): stored as `OptedIn` (`store.rs`), missing = off; the old `Disabled` key is ignored.
  - `apps/desktop/src/error-reporting.ts`: replay integration with `maskAllText`, `maskAllInputs`, `blockAllMedia`, no network detail or bodies; starts only with crash consent and the replay opt-in.
  - Settings → Privacy (`apps/desktop/src/settings/privacy/index.tsx`): "Share usage data" (opt-in), "Error", and a new "Session replay" switch, each off by default.
- **Migration choice:** reset to off for everyone. A stored `telemetry_consent = true` may be a default written by an older build, so it can't count as an opt-in. Cost: a user who had explicitly opted in must opt in again. No database migration.
- **Redaction when on:** unchanged and already central in `crates/analytics/src/lib.rs` (`sanitized_properties`, `is_safe_analytics_string`): content-like keys dropped, strings with spaces or longer than 96 characters dropped. GAP: a one-word value under an innocuous key would pass the sink's filter; call sites never pass content (C4.2 marker test below).
- **Tests:** `analytics_is_off_unless_the_user_opted_in` (`plugins/analytics/src/ext.rs`), `enables PostHog only on an explicit opt-in` and `never turns analytics on from the legacy telemetry setting` (`settings/queries.test.tsx`), `stays off with error reporting on until the user opts in, then masks everything` (`error-reporting.test.ts`), `settings/privacy/index.test.tsx`.

### C6.1 recording indicator: EXISTS (was PARTIAL)

- `plugins/tray/src/ext.rs`: `set_visible` stores the `show_tray_icon` preference (`USER_WANTS_VISIBLE`) and `apply_visibility` shows the icon when `tray_icon_visible(preference, recording)`; `set_recording` reapplies it, so the animated menu-bar icon appears for every recording even with "Show tray icon" off and hides again afterwards. The settings copy says so (`settings/general/app-settings.tsx`).
- The floating bar (`floating_bar_enabled`) stays optional; it is an extra indicator.
- Test: `the_recording_indicator_ignores_the_hide_setting`.
- GUESS: `set_recording(true)` is called on the listener's `Active` lifecycle event (`plugins/transcription/src/listener/runtime.rs:51`); if audio capture can begin before that event, the icon appears a moment after capture starts. Not measured.

### C6.5 calendar auto-start: EXISTS (was GUESS)

- `apps/desktop/src/stt/scheduled-auto-start-notice.ts`: `notifyScheduledRecordingStarted` shows a system notification ("Recording started automatically"), no meeting title, ignoring the in-app notification preferences; called after the scheduled `startListening` resolves (`scheduled-session-auto-start.tsx`). The menu-bar indicator comes from C6.1.
- Test: `scheduled-session-auto-start.test.tsx` (the notification on a scheduled start, none on a manual start).
- GAP: macOS can still suppress the notification (Focus / Do Not Disturb or notifications denied for the app); the menu-bar icon is then the only signal.

### A4.1 permissions and C2.1 FileVault check: EXISTS (was MISSING)

- **Core** `crates/aspc/src/protection.rs`: `parse_fdesetup_status`, `filevault_status` (`/usr/bin/fdesetup status`), `is_protected_file_name`, `harden_app_data` (dir `0700`, `*.db`/`*.sqlite` with `-wal`/`-shm`/`-journal` and `*.json` directly in the dir `0600`; only clears bits; symlinks skipped).
- **Startup** `apps/desktop/src-tauri/src/data_protection.rs` (`spawn`, called from the app's `setup` after the retention job): one background thread tightens the data dir and logs `app_data_permissions_tightened` with path and modes only, then logs `filevault_on` / `filevault_off` / `filevault_status_unknown`. The result is cached for `get_data_protection_status` (registry `app`, `Public`: a device fact).
- **UI:** a warning toast once per launch when FileVault is off (`apps/desktop/src/data-protection/filevault-warning.tsx`, mounted in `main/lifecycle.tsx`), never blocking; Settings → Privacy shows "Disk encryption (FileVault)" On / Off / Unknown (`disk-encryption-row.tsx`).
- **Observed on this Mac (read-only listing, not changed):** the dev data dir is `drwxr-xr-x` and `app.db*` / `store.json` are `-rw-r--r--`; `audit.db*` is already `0600`. The next launch should log the tightening.
- **Tests:** `parses_fdesetup_output`, `protected_names_cover_databases_and_stores`, `startup_tightens_the_data_dir_and_its_files` (`crates/aspc`), `filevault-warning.test.tsx`.
- **Gaps:** the vault can be moved to a custom folder outside the app data dir (`settings` vault base); that folder's mode is not changed. Files created later by SQLite take the database file's mode, so they stay `0600`; other new JSON files are fixed at the next launch. SQLCipher: deferred gate C2.3.

### C5 backup and restore: EXISTS (plaintext; was MISSING)

- **Core** `plugins/session/src/backup.rs`: `write_backup` (work folder next to the target, `VACUUM INTO`, meeting count, `collect_files` skips symlinks, tar written to `archive.partial` then renamed, `0600`), `allowed_entry`, `check_database` (read-only, `integrity_check`, `sessions` table), `stage_restore` (unpack into a temp folder in the data dir, manifest format check, marker with the vault's `sessions/` path from the settings, rename to `restore-pending/`), `apply_pending_restore` (moves `app.db`, `-wal`, `-shm` and `sessions/` to `*.<UTC time>.pre-restore`, moves the restored ones in; copies across volumes for a custom vault).
- **Commands** `plugins/session/src/commands.rs` (`backup_library`, `restore_library`, `confirm` native dialog via `tauri-plugin-dialog`, `database_dir` from `pragma_database_list`): `aspc::require` first, then `fs2().authorize_write` / `authorize_read` (dialog grant, single use), then the Rust confirmation, then the work, then the audit entry. Registry `crates/aspc/src/commands.rs` (session block); capability `apps/desktop/src-tauri/capabilities/main.json` only (`session:allow-backup-library`, `session:allow-restore-library`; not in `session:default`, so the composer doesn't get them).
- **Startup** `apps/desktop/src-tauri/src/db.rs` (`open_desktop_db`): applies a staged restore right after the existing reset marker, before the pool opens.
- **UI** `apps/desktop/src/settings/privacy/backup.tsx`: "Back up library…" / "Restore from backup…"; after staging a toast offers "Restart now" (refused while recording).
- **New dependencies** (plugin `tauri-plugin-session`): `tar` 0.4 (MIT OR Apache-2.0, already in `Cargo.lock`), `tauri-plugin-dialog` and `tauri-plugin-fs2` (workspace), `tempfile` moved from dev to normal. `Cargo.lock` gains three dependency edges, no new crates.
- **Tests:** `backup_restores_into_a_clean_library` (two meetings with a transcript, audio, an attachment and a memo; wipe; stage; nothing changes before the restart; apply; rows and file bytes match; the replaced library is kept; a second apply is a no-op), `hostile_or_foreign_archives_are_refused_and_change_nothing` (`..`, absolute path, unknown file, symlink, non-SQLite `app.db`, newer format), `settings/privacy/backup.test.tsx`.
- **Gaps:**
  - not encrypted (C5.1's default needs `age` or similar: a dependency decision for the user);
  - the `sessions/` copy is not a snapshot: a recording in progress during a backup may be copied half-written (the database snapshot is consistent);
  - a restore on another Mac brings the database but not the Keychain items (Engine key, E2EE recovery key, calendar tokens), and the restored library keeps its workspace binding; sync after such a restore was not tested;
  - `backup.restored` is recorded when the restore is staged, not again when it is applied at launch;
  - the vault `sessions/` path is taken when staging; if the vault moves before the restart, the restored folder goes to the old place;
  - not run in CI as a separate job (C5.3): it runs with the `tauri-plugin-session` tests.

### D4.1 / D4.2 / D6.3 privacy surfaces: EXISTS (links are placeholders)

- **Placeholders** `apps/desktop/src/settings/privacy/legal.tsx`: "Privacy Policy" and "Terms of Use" are disabled buttons labelled "(placeholder, not written yet)"; `PRIVACY_POLICY_URL` and `TERMS_URL` are `null`. **TODO (owner: not assigned; date: not set):** write the privacy policy and terms, then set both URLs and open them with the opener plugin.
- **"Where your data lives"** `apps/desktop/src/settings/data-locations/index.tsx` (nav: Advanced → "Where your data lives", `sidebar/settings-nav-groups.ts`; tab id `data-locations` in `store/zustand/tabs/schema.ts`): on this Mac (meetings folder, database, search index, audit log, logs, Keychain, FileVault state), what leaves it (Anthropic through the Engine with the user's key and the pinned Haiku model; transcription per the chosen provider; calendar read-only; sync only when turned on; update check; usage data and error reports per the opt-ins), and the "never" statement.
- **Paths** `app:get_data_locations` (`apps/desktop/src-tauri/src/data_protection.rs`, guarded `(Settings, Read)`): `desktop_db_dir` for `app.db` / `audit.db`, the settings vault base for `sessions/` and `search_index/` (the Tantivy plugin joins `search_index` to the vault base), `app_log_dir` for logs.
- **Tests:** `settings/data-locations/index.test.tsx` (paths shown, lines follow the settings).
- **To check by a person (not verified in code):** the sync line says "end-to-end encrypted copy"; meeting rows sync as `e2ee_*` ciphertext (section 1), but whether every synced item, cloud attachments in particular, is end-to-end encrypted was not checked. The Anthropic training statement paraphrases Anthropic's commercial API terms and should be confirmed by whoever owns legal review before release.

### D5.1 licences screen: EXISTS; D5.3 flags: NEED A DECISION

- **Generator** `scripts/generate-licenses.mjs` (`classify` handles SPDX `OR` / `AND` / `WITH` and old `A/B`; test `scripts/generate-licenses.test.mjs`, added to the `node --test` list in `.github/workflows/ci.yaml`). Output `apps/desktop/src/settings/about/third-party-licenses.gen.json`: the `LICENSE` text, 1018 crates, 1049 npm packages, and `flagged`. `--check` is not run in CI yet (the `ci.yaml` job has no Rust toolchain).
- **UI** Settings → About (`apps/desktop/src/settings/about/index.tsx`, nav Advanced → About, tab `about`): the MIT licence verbatim, then Rust crates and JavaScript packages with their licences, filterable; the JSON is loaded only when the page opens.
- **Flagged on 2026-11-17 (facts from package metadata; none approved):**
  - **LGPL-3.0 (Rust, linked into the app):** `mp3lame-encoder` 0.2.3, `mp3lame-sys` 0.1.11 (LAME MP3 encoder, statically linked). LGPL in a statically linked, closed or MIT app has relinking obligations: **needs a decision** (replace, make it dynamic, or accept with the LGPL obligations met).
  - **LGPL (npm):** `pos` 0.4.2: **needs a decision**.
  - **MPL-2.0 (file-level copyleft; usually fine unmodified, keep notices):** `symphonia` and its codec/format crates 0.5.5, `cssparser` 0.29.6 / 0.36.0, `cssparser-macros`, `selectors` 0.24.0 / 0.36.1, `dtoa-short`, `option-ext`, `progenitor-client`; npm `lightningcss` 1.30.1 (+ `lightningcss-darwin-arm64`).
  - **No licence in the package metadata (npm):** `@giscus/react` 3.1.0, `@splinetool/runtime` 0.9.526, `khroma` 2.1.0. GUESS: `khroma` and `@giscus/react` are MIT in their repositories, but the shipped package doesn't say so; `@splinetool/runtime` is a proprietary runtime as far as I know (not verified). **Needs a decision.**
- **Gaps:** licence names only, not the full notice texts (Apache-2.0 `NOTICE` files are not collected); the generator covers macOS dependencies only.

### C4.2 marker test: EXISTS (partial coverage); two leaks fixed

- **Tests** (marker `zqxcontentmarker4417`): `meeting_content_reaches_no_log_or_error` (`plugins/session/src/content_marker_tests.rs`; a global `tracing` subscriber at `info`, like `plugins/tracing`, captures every thread; retention deletes the old marked meeting, the purge removes both, a backup is written, a missing and a hostile archive fail, an audit entry with the title is refused; asserts on logs, errors and the `audit.db` bytes), `provider_errors_never_carry_meeting_content` (`crates/ai-engine/src/tests.rs`, statuses 400/404/413/422/429/500), `meeting_content_markers_never_reach_analytics` (`crates/analytics/src/lib.rs`), `denials_never_carry_the_statement_text` (`crates/db-core/src/webview_sql.rs`), `webview_console_content_never_reaches_the_log` (`plugins/tracing/src/ext.rs`), `content marker` in `apps/desktop/src/error-reporting.test.ts`, `apps/desktop/src/ai/engine/errors.test.ts`.
- **Leaks found and fixed:**
  - the Engine kept the provider's error message for 4xx errors other than 401/403/429 (`map_status`, `crates/ai-engine/src/lib.rs`); a provider message can quote the request. It now keeps only the provider's error type when it is an identifier.
  - the analytics sink kept a one-word string under a key such as `meeting_title` (only exact sensitive keys and `_id`/`_email`/`_url`/`_path` suffixes were dropped). `is_content_property_key` now drops string values under any key containing a content word (`title`, `transcript`, `note`, `content`, `prompt`, `summary`, `message`, `query`, `text`, `body`, …); numbers and flags such as `word_count` or `had_query` stay.
- **Observed, not changed:** `sqlx` logs full statement text at `debug` (and slow statements at `warn`); Rust code binds values, so content appears only if a statement embeds a literal. The app logs at `info` unless `RUST_LOG` is set. The `[cloudsync]` console allowlist keeps the first argument up to 200 characters, so `console.error("[cloudsync] … " + content)` would be logged (no such call was looked for).
- **Not covered yet:** live capture and transcription, Tantivy search errors, PDF/Markdown export failures, Ask tool errors, and the real `app.log` writer (the test uses an equivalent subscriber, not `plugins/tracing`'s file layer and redaction).

### C3.1 / C7.7 check-secrets: fresh database and release wiring: EXISTS

- `scripts/check-secrets.mjs`: `--fresh-db` (`withFreshDatabase`: temp folder, `cargo run --locked -p db-app --example fresh_db`, scan `app.db` / `-wal` / `-shm` as bytes, delete) and `--db <file>`; `scanDatabaseFiles` allows no fake keys. `crates/db-app/examples/fresh_db.rs` opens a new database, runs `prepare_schema` (migrations and seed templates) and checkpoints. Test: `a key stored in a database file fails, even inside binary pages` (`scripts/check-secrets.test.mjs`).
- **Run on this Mac:** `scripts/check-secrets --fresh-db` → sources scanned, fresh database scanned, 0 findings.
- **CI:** `cli_ci.yaml` runs `--fresh-db` after the db-app tests (path filter now includes `scripts/check-secrets.mjs`); `desktop_cd.yaml` runs `--require-bundle` after the macOS, Linux and Windows `tauri build` steps.
- GUESS: the release bundle embeds `VITE_POSTHOG_API_KEY` and `VITE_SUPABASE_ANON_KEY` (public client keys); neither matches the key patterns (`sk-ant-`, `sk-`, `AKIA`/`ASIA`, `AIza`), so the release step should pass. Not run against a real release bundle.

### A2.3 command guard: 10 more commands guarded (224 `Pending` remain)

- **Guarded now** (`aspc::require` first; registry `crates/aspc/src/commands.rs`): `fs-sync:delete_folder`, `fs-sync:folder_attachment_remove` `(Notes, Delete)` (`plugins/fs-sync/src/commands.rs`); `session:tombstone_session_audio` `(Recording, Delete)`, `session:tombstone_folder_material`, `session:delete_folder_catalog` `(Notes, Delete)` (macro calls replaced by explicit functions, `plugins/session/src/commands.rs`); `transcription:delete_voice_enrollment` `(Recording, Delete)` (`plugins/transcription/src/voice_enrollment.rs`); `accounts:connect_account` `(Keys, Create)`, `accounts:disconnect_account` `(Keys, Delete)` (`plugins/accounts/src/commands.rs`; a denial reaches the webview as `{ code: "aspc.*", message }`, `From<Denied>` in `src/lib.rs`; new dependency edge `anlg-aspc`); `app:install_agent_skill`, `app:install_embedded_cli` `(Settings, Administer)`. Plus the Phase 5 commands `session:backup_library`, `session:restore_library`, `app:get_data_locations` (new, guarded).
- **How they were chosen:** deletes, key changes and installs whose only callers (checked by grep) are user actions in the main or note windows: folder delete (`sidebar/folder-materials.tsx`, `folders/folder-editor.tsx`), folder material delete, session audio delete (`audio-player/provider.tsx`, `session/move-contents.ts`), voice profile delete (Settings), calendar account connect/disconnect (calendar settings), developer installs (Settings → Developers).
- **Left Pending on purpose (risk of breaking a background path):** deletes that also run from background flows (`fs-sync:delete_session_folder` from sync evictions, `fs-sync:audio_delete` from capture cleanup, `fs-sync:attachment_remove` from screen-record cleanup, `calendar:tombstone_calendar_connection`), the E2EE key commands (`db:create_e2ee_identity`, `import_*`, `seal_*`, called by automatic sync enrollment and possibly while the app is locked), `db:configure_cloudsync_token` (token refresh), `misc:allow_http_origin` (also called by `enterprise-capture`), and macro-generated commands whose functions the registry test can't see (`calendar:soft_delete_contact`).
- **Tests:** `tests/command_registry.rs` (every guarded entry calls the guard), `state::tests::guard_denies_locked_viewer_and_wrong_window` (every registry entry). Not run: the `tauri-plugin-fs-sync` and `tauri-plugin-transcription` test suites (`cargo check -p desktop` compiles them).

### Phase 5 test results (2026-11-17, this Mac)

- **Rust:** `db-execute --test workspace_guard` 9 (new `only_final_denials_reach_the_denial_sink` fails on the Phase 4 code); `db-core --lib webview_sql` 11; `db-reactive` all pass; `aspc` 23 unit + 2 registry + 2 role scans; `tauri-plugin-analytics` 4; `tauri-plugin-anlg-tray` 16; `tauri-plugin-session` 46 (backup 2, content marker 1); `tauri-plugin-accounts` 1; `ai-engine` 23 + 2; `analytics` 4; `tauri-plugin-tracing` console test; `desktop --lib` 35. `cargo check --locked -p desktop` passes. `cargo clippy --all-targets --no-deps -D warnings` is clean for `aspc`, `db-core`, `db-app` (with the new example), `db-execute`, `ai-engine`, `tauri-plugin-session`, `tauri-plugin-accounts`. Clippy fails on existing code in `analytics` (`is_safe_analytics_string`, `nonminimal_bool`) and `tauri-plugin-anlg-tray` (`rebuild_menu`, `refresh_menu_if_agenda_changed`); not changed here.
- **Desktop:** `tsc --noEmit` passes; `oxlint` 0 errors (225 warnings, as before); full `vitest run`: 503 of 504 files, 4422 of 4424 tests pass; the 2 failures are the known date-dependent `settings/general/billing.test.tsx` cases. i18n: `lingui extract --clean` and `compile --strict` are stable.
- **Scripts:** the `ci.yaml` `node --test` list: 102 of 104 pass; the 2 failures (`desktop-release-provenance.test.mjs`, Store package cases) also fail without these changes. `scripts/check-secrets --fresh-db`: 0 findings. `node scripts/generate-licenses.mjs --check`: up to date.
- **Not run:** `tauri-plugin-fs-sync`, `tauri-plugin-transcription`, `tauri-plugin-db` test suites (compiled by `cargo check -p desktop`); the app itself.

### Phase 5 runtime check

1. **Denials at startup:** launch, then Settings → Audit: no `permission.denied` / `sql.workspace` entries for `events`, `json_each` or `local_recording_import_jobs` from the launch. Locking the app with a note open still records a few denials.
2. **Defaults:** on this existing install, Settings → Privacy shows "Share usage data", "Error" and "Session replay" all off (this install's earlier consent is ignored on purpose). Turn "Share usage data" on and off; nothing else changes.
3. **Recording indicator:** Settings → App → turn "Show tray icon" off; start a recording: the menu-bar icon appears and animates; stop: it disappears again. With the floating bar off as well, the icon is still there.
4. **Calendar auto-start:** with "Auto-start scheduled meetings" on and the window hidden, let a calendar meeting with a link start: a macOS notification "Recording started automatically" appears (no meeting title) and the menu-bar icon animates.
5. **Permissions and FileVault:** after the first launch, `ls -la` of the data folder shows `drwx------` and `-rw-------` for `app.db*`, `audit.db*` and `store.json`; `app.log` has `app_data_permissions_tightened` lines with paths and modes only. If FileVault is off, a "FileVault is off" toast appears once; Settings → Privacy shows "Disk encryption (FileVault)".
6. **Backup:** Settings → Privacy → "Back up library…", pick a place: a native dialog warns it is not encrypted; after OK a `.anarlogbackup` file appears (mode `0600`) and Settings → Audit shows `backup.created` with counts.
7. **Restore:** "Restore from backup…", pick that file: a native confirmation; after OK, "Restore ready" with "Restart now". Restart: the library matches the backup; the data folder has `app.db.<time>.pre-restore` and `sessions.<time>.pre-restore`; Settings → Audit shows `backup.restored`. Try a renamed non-backup file: "Restore failed", nothing changes.
8. **Where your data lives:** Settings → Advanced → "Where your data lives": the paths match the real data folder; the transcription line names the chosen provider (or says audio stays on the Mac); the usage-data and error-report lines follow the switches.
9. **Legal placeholders:** Settings → Privacy shows "Privacy Policy (placeholder, not written yet)" and "Terms of Use (…)" as disabled buttons.
10. **About:** Settings → Advanced → About shows the MIT licence and the Rust and JavaScript lists; the filter works.
11. **Guarded commands:** delete a folder, a folder file, a meeting's audio and the voice profile; connect and disconnect a calendar account; all still work from the main window.

## Not verified

- No runtime test was run for any of this; it's a read-only audit.
- The two GUESSes on the most important path: the plain `-wal`/`.bak` copies of old keys, and whether hidden-window auto-start shows any indicator.

## Proposed design for A1, A2 and C1

A1 and A2 were built in Phase 2a (see "Phase 2a changes"), with these differences: the guard sits in 42 commands, not ~120; the per-window limits are also enforced in Rust; the lock has a narrow lock-screen exception. C1 was built in Phase 2b (see "Phase 2b changes" and "C1 decision"): the webview part ships as the single-workspace guard, not views.

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

1. ~~**Efficiency Engine (C7, verification item 10).**~~ **Decided 2026-11-16: LOCAL Direct mode, proxy-ready; the Engine replaces provider and model choice for LLM features.** Built in Phase 3; see ASPC_BASE "Decisions" and gate C7.6. Original proposal:
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
   - ~~Tantivy schema fields `workspace_id` and `session_id`;~~ approved and built (Phase 2b);
   - ~~a data backfill of empty workspace and owner ids (C1).~~ approved and built as a startup step (Phase 2b).

   Alternative for Private: reuse the existing `locked` flag to mean Private, with no new column.
4. **Defaults that change behaviour:**
   - `telemetry_consent` and Session Replay are on by default today. I propose off (D6, C4.3).
   - `auto_start_scheduled_meetings` is on by default today, and indicators can be hidden. I propose forcing an indicator during recording (C6.1), and leaving auto-start on but with a notification.
5. **Soft delete (D2.2).** Deleting a meeting would become a permanent delete after the 5-second undo, instead of keeping tombstoned rows forever. Sync relies on tombstones; for synced meetings I'd keep a tombstone row with the content columns blanked.
6. ~~**Hooks feature.**~~ **Decided 2026-11-16: removed.** See ASPC_BASE "Decisions".
