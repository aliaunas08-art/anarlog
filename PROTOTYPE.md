# Granola-style prototype notes (fork of fastrepl/anarlog)

Working notes for the `prototype` branch. **F** = verified in code, **I** = inference.
Paths are relative to the repo root.

## Phase 1: Understand

### Stack

- Desktop app: Tauri v2 (`apps/desktop`), React + TypeScript + Vite + Tailwind UI,
  Rust backend split into `plugins/*` (Tauri capabilities) and `crates/*` (libraries).
- Local data: SQLite (`app.db`), schema and migrations in `crates/db-app`.
- Package managers: pnpm 11.1.1 workspace + Cargo workspace (Rust 1.94.0, pinned in
  `rust-toolchain.toml`).
- Optional hosted backend: `apps/api` (Rust/Axum) + `supabase/` + Stripe. Not needed for the prototype.

### Commands (from README / CONTRIBUTING / AGENTS.md)

| Purpose | Command |
| --- | --- |
| Install | `pnpm install --frozen-lockfile` |
| Desktop dev | `pnpm dev:desktop` (process-compose: builds `@anlg/ui`, then `pnpm -F @anlg/desktop tauri:dev`) |
| Typecheck TS | `pnpm -F @anlg/desktop typecheck` |
| Desktop tests | `pnpm -F @anlg/desktop test` |
| Rust check | `cargo check --locked -p <crate>` |

### System requirements (macOS, Apple silicon)

- Node.js 22+ (the `engines` field requires >= 24.22.1 on the 24 line), pnpm 11.1.1, Rust 1.94.0
- process-compose >= 1.122.0
- **Full Xcode with the Metal Toolchain component.** The Command Line Tools alone fail:
  `crates/transcribe-soniqo/build.rs` panics if it cannot find a Metal compiler (it compiles the
  MLX shader library for on-device Soniqo transcription).
  After installing Xcode, run:
  `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcodebuild -downloadComponent MetalToolchain`

### Architecture map

- **Audio capture (F):** `plugins/transcription` → `crates/listener-core` (actor system) →
  `crates/audio-actual`. Mic via cpal (`audio-actual/src/mic.rs`). System audio via a CoreAudio
  process tap (`audio-actual/src/speaker/macos.rs`). No bot: everything is captured on the device.
  The channel mode is chosen in `crates/listener-core/src/actors/mod.rs` (`ChannelMode::determine`):
  always `MicAndSpeaker` except during onboarding. `MicOnly` exists but is unused.
- **Recording to disk (F):** `listener-core/src/actors/recorder/chunks.rs` streams MP3 to
  `<data>/sessions/<id>/audio.mp3`, plus rolling 60 s recovery chunks (`audio-recovery/*.part`).
  It writes and `fsync`s every 1 s and recovers leftover chunks on the next launch.
- **Transcription:** local options are Soniqo (Parakeet, MLX, download needed), Apple Speech
  (macOS 26+, no download) and whisper.cpp (`plugins/local-stt`). Cloud options use your own key:
  Deepgram, AssemblyAI, OpenAI and others (`owhisper-client`). The hosted "Anarlog/Pro" STT needs a
  paid account. Speakers come from mic/system channel separation plus diarization (on-device
  pyannote for local batch runs, `crates/listener2-core/src/batch/diarize.rs`).
- **Summary prompt/templates (F):** prompts are Askama/Jinja files in `crates/template-app/assets/`
  (`enhance.system.md.jinja`, `enhance.user.md.jinja`, `enhance.format.md.jinja`). Templates are
  rows in the SQLite `templates` table. The built-in templates are seeded by migration
  `crates/db-app/migrations/20260524000000_default_templates.sql`.
- **Storage (F):** SQLite `app.db` (`sessions`, `session_documents`, `transcripts`, ...) under
  `~/Library/Application Support/com.hyprnote.dev/` for dev builds (`crates/storage/src/global.rs`).
  Audio and attachments are plain files next to it.
- **Sync (F):** CloudSync is opt-in. It wraps the sqliteai `sqlite-sync` extension
  (`crates/cloudsync`) with end-to-end encryption (`crates/e2ee`) and needs the hosted API.
- **LLM (F):** Settings → Intelligence supports Anthropic with your own key
  (`apps/desktop/src/settings/ai/llm/shared.tsx`). The key is stored in the macOS Keychain via
  `store2.setSecret`. Model list is fetched live from Anthropic's `/models`.

### Features that only work through Anarlog's hosted cloud

The server code is in the repo, but these need Anarlog's hosted deployment or your own
Supabase/Stripe/Nango setup:

- "Pro (Cloud)" managed AI: the LLM proxy (Claude via OpenRouter) and STT proxy (Soniox), `apps/api/src/routes/ai.rs`
- Anarlog's speech-to-text option (model `cloud`), which requires login and a paid plan
- CloudSync (encrypted sync across devices) and the account system
- Note sharing and shared-note web pages (paid plan)
- Nango-backed integrations: Google/Outlook calendar, Gmail, Notion, Zoom, CRM
- Template suggestions fetched from `https://anarlog.so/api/templates`
- The UI gates the "markdown-export" automation behind the Pro plan. Manual export is free.

Everything the prototype needs works locally: capture, local or BYO-key STT, BYO-key Claude,
SQLite storage, manual Markdown export, Apple Calendar.

## Phase 2: Build and run

### One-time machine setup (done on this Mac, outside the repo)

| Tool | How | Where |
| --- | --- | --- |
| Rust 1.94.0 | rustup official installer, toolchain from `rust-toolchain.toml` | `~/.cargo` |
| Node 24.22.3 | nodejs.org tarball (SHA256 verified) | `~/.local/opt/node` |
| pnpm 11.1.1 | corepack shim | `~/.local/bin/pnpm` |
| process-compose 1.124.0 | GitHub release (SHA256 verified) | `~/.local/bin` |
| cmake 4.5.0 | Kitware release (SHA256 verified) | `~/.local/opt/cmake` |
| Xcode 27.2 + Metal Toolchain | App Store; `xcodebuild -downloadComponent MetalToolchain` | `/Applications/Xcode.app` |

### Build fixes

1. `Cargo.toml`: `strip = false` for dev dependencies. macOS 27 refuses proc-macro dylibs stripped by Rust < 1.98 (rust-lang/rust#157750). Committed.
2. `plugins/permissions/swift/check-permissions.swift`: `import ApplicationServices`. The Xcode 27.2 SDK no longer re-exports `AXIsProcessTrusted`. Committed.
3. `apps/desktop/.env` (gitignored, local only) sets `CARGO_TARGET_DIR=~/.cache/anarlog-target`. The repo path contains a space ("Granola Prototype"), which breaks libtool in `mp3lame-sys`.
4. `~/.local/bin/swift` (local shim, not in repo). Xcode 27's SwiftPM defaults to the new `swiftbuild` engine, which puts libraries where the `swift-rs` crate cannot find them. The shim adds `--build-system native` to `swift build`.
5. `cmake` installed (needed by whisper.cpp and pyannote's `knf-rs-sys`).

### Run it

From a new Terminal window:

```bash
cd "$HOME/Documents/Claude/Granola Prototype/anarlog"
export PATH="$HOME/.local/bin:$HOME/.local/opt/node/bin:$HOME/.cargo/bin:$PATH"
pnpm dev:desktop
```

A warm start takes about a minute. Logs are written to `.process-compose/desktop/desktop.log`.
To make the `PATH` line permanent, add it to `~/.zprofile`.

### macOS permissions

macOS asks the first time each one is needed. In dev mode it may attribute the request to
the app that launched the dev server (Terminal, or Claude) rather than "Anarlog Dev". If a toggle is missing,
add or enable that app.

| Permission | Needed for | Where |
| --- | --- | --- |
| Microphone | All recording, including in person | System Settings → Privacy & Security → Microphone |
| System Audio Recording | Online meetings (other participants' audio). Not needed for in person | System Settings → Privacy & Security → Screen & System Audio Recording → "System Audio Recording Only" |
| Screen Recording | Optional meeting screenshot context only | Same pane, top section |
| Accessibility | Optional automatic meeting detection only | Privacy & Security → Accessibility |

## Phase 3: Prototype changes

### A. "Meeting Minutes" template

- Seeded by migration `crates/db-app/migrations/20261114120000_meeting_minutes_template.sql`. This is a data-only
  `INSERT OR IGNORE` with no schema change, the same mechanism as the existing default templates. It is pinned, so it appears at the top of the template picker.
- Sections, in order: Meeting Details (title, date, attendees) → Summary (3–5 sentences) → Key Decisions →
  Action Items (table `Action | Owner | Due date`, "Not stated" when missing, never inferred) → Open Questions →
  Transcript.
- The template's rules tell the model that typed Meeting Notes win when they conflict with the transcript. The shared system prompt already
  says template instructions take precedence over formatting defaults (`enhance.system.md.jinja`).
- **Decision:** the LLM does not re-type the full transcript. That would be slow and expensive, and long meetings could be cut off. The
  transcript stays separate in the app's Transcript tab (with speaker labels). Markdown export now includes it
  by default (`export_include_transcript` default `true`).
- **LLM:** Settings → Intelligence → Anthropic → paste your API key (stored in the macOS Keychain) → pick a Claude
  model. Nothing is hardcoded.

### B. In-person (mic-only) mode

- **Before this change:** you could record with no meeting (open a note → Record), but it always captured mic plus system audio.
  `ChannelMode::MicOnly` existed in `crates/listener-core` but was never used.
- **Change:** a new **"In person"** button next to Record in the note header. It starts the same pipeline with
  `mic_only: true` → `ChannelMode::MicOnly`. It opens only the microphone, never the system-audio tap, and skips meeting
  chat capture, screen capture and the recording disclosure. The microphone is never marked "personal", so people in
  the room are not all labelled "You".
- Resume and restart of an in-person session stay mic-only for the rest of the app run (in memory only, since saving it would need a schema change).
- **Known limitations (inference, not verified with live audio):** live captions may label all room speech as
  one speaker. Per-person labels come from the diarization pass after recording. The recorder stores mono as two identical
  channels. Local transcription collapses them; a hosted batch STT provider could return duplicate words.

### C. Reliability (verified in code, no change needed)

`crates/listener-core/src/actors/recorder/chunks.rs` writes MP3 to disk **during** recording, independent of
transcription or network:

- `~/Library/Application Support/com.hyprnote.dev/sessions/<session-id>/audio.mp3` (dev build)
- 60 s recovery chunks in `.../audio-recovery/*.part`, flushed with `fsync` every 1 s; recovered on next launch after
  a crash. Storage errors retry every 5 s with up to 64 MB buffered in memory.

### D. Markdown export (existing feature, no code change except the default)

- Pick the folder: Settings → General → Storage → **Export location** → Choose folder (default: Downloads).
- Per meeting: note header → `…` menu → Export → **Markdown** → tick Summary (and Memo/Transcript) → Export. The file is
  written as `<title>_<timestamp>.md`.
- The automatic per-meeting markdown export ("markdown-export" automation) is gated behind the Pro plan in the UI. Manual
  export is free.

## Phase 4: Verification checklist

| # | Check | Result | Evidence |
| --- | --- | --- | --- |
| 1 | App builds and launches with no errors | **PASS** | `cargo build --features dev` finished (`Finished dev profile … 5m 30s`); `pnpm dev:desktop` opened the "Anarlog Dev" window (user screenshot, 16:41). No Rust panics in `.process-compose/desktop/desktop.log`. The only stderr "errors" are `[perf] slow …` console warnings from the UI. |
| 2 | Mic recording → transcript with speaker labels | **PASS** (single speaker) | User test 2026-11-14 with the In person button and Apple Speech. App log: `start_source_loop new_mode=MicOnly`. Transcript tab shows the words under "Speaker 1". A two-person recording to confirm per-person labels is still open. |
| 3 | Meeting Minutes has every section; no invented owners/dates | **PASS** | Session "March Launch Planning" (Claude via your own key): all 6 sections in order; action table: Sam, Priya with stated dates; pricing owner "Not stated"; onboarding testing owner and date "Not stated". Notes beat transcript: after adding the memo "Deadline for the deck: Friday" and regenerating, Sam's due date changed from Thursday (the transcript) to **Friday** (the note). |
| 4 | Audio file exists on disk after stopping | **PASS** | `~/Library/Application Support/com.hyprnote.dev/sessions/5b61e32e…/audio.mp3` (10.3 s) and `sessions/7e7de75d…/audio.mp3` (32.0 s), checked with `afinfo`. |
| 5 | Markdown export opens and matches the app | **PASS** | `~/Downloads/March Launch Planning_2026-11-14T22-48-00-823Z.md`: the same sections and table as the screen, plus `## Transcript` with speaker labels. Minor: "Duration: 0m" for a 32 s recording; the minutes' h1 headings sit under the export's `## Summary`. |

Automated checks run on this branch:

- `cargo test --locked -p db-app -p listener-core`: 278 + 142 passed, 0 failed (includes the new
  `channel_mode_tests` and the updated template-seed tests).
- `cargo test --locked -p tauri-plugin-transcription`: 51 passed, 0 failed.
- `pnpm -F @anlg/desktop typecheck`: clean. `oxlint`: 0 errors.
- Vitest: `export-modal.test.tsx` 19/19; `src/stt` + `outer-header` 206/206.
- Pre-existing failures, unrelated (fail on a clean checkout too): `settings/general/billing.test.tsx` (2 tests).
  `stt/primary-device.test.tsx` is timing-flaky under full-suite load (passes 3/3 in isolation).

### Manual test steps

**Setup (once)**
1. Start the app (Phase 2 → Run it).
2. Settings → **Transcription** → choose a local model. **Apple Speech** needs no download. **Soniqo Parakeet**
   downloads about 120 MB. Wait until it shows as ready.
3. Settings → **Intelligence** → **Anthropic** → paste your API key → choose a Claude model (for example Claude Sonnet 5.5).
4. Settings → General → Storage → **Export location** → Choose folder → pick a test folder.

**Test 2 + 4: in-person recording**
1. Create a new note (⌘N). In the header you should see **Record** and **In person**. Take a screenshot as evidence.
2. Type a few notes, including one fact that contradicts what you will say (for example the note "Deadline: Friday" while you say "Thursday").
3. Click **In person**. Allow Microphone if macOS asks.
4. Talk for about 2 minutes with a second person if possible. Include one clear task with an owner and date ("Sam will send the deck by
   Friday") and one task with no owner or date ("someone should look at pricing").
5. Click **Stop**.
6. Open the **Transcript** tab. PASS if the text is present and speakers are labelled. Per-person labels may only appear
   after the post-recording pass finishes.
7. Audio on disk (PASS if an `audio.mp3` from now is listed):
   ```bash
   ls -lt ~/Library/Application\ Support/com.hyprnote.dev/sessions/*/audio.mp3 | head -3
   ```

**Test 3: Meeting Minutes**
1. In the same note, open the template picker → **Meeting Minutes** (pinned at the top) → generate.
2. PASS if the headings appear in order: Meeting Details, Summary, Key Decisions, Action Items (a table),
   Open Questions, Transcript.
3. PASS if the action items table lists "Sam … Friday" and shows **Not stated** for the pricing task's owner and date.
4. PASS if the conflicting fact follows your typed note, not the transcript.

**Test 5: Markdown export**
1. Note header → `…` → **Export** → **Markdown** → keep Summary and Transcript ticked → Export.
2. Open the `.md` file in the chosen folder. PASS if the minutes match the note on screen and the transcript appears under
   `## Transcript`.

**Optional: online meeting (no bot)**: open a call, click **Record** (not In person), and allow System Audio Recording.
The other side's speech should appear as separate speakers.

## Connect Google and Microsoft (on the Mac, no server)

The app signs in to Google and Microsoft directly: the browser opens the provider's consent page, the provider sends the
browser back to a one-time page the app serves on this Mac (`127.0.0.1` / `localhost`), and the app keeps a refresh token in
the macOS Keychain (item service `com.hyprnote.dev.accounts` for the dev build). Access is read-only: who you are, plus your
calendars. Nothing goes through an app server.

Each provider needs an app registration that you own. Both are free. The client IDs are not secret for a desktop app, but
they still stay out of the code: they go in `apps/desktop/.env` (gitignored), and a release build can take them from the
build environment instead.

The console menu names below are as of late 2026 and may move.

### Google (about 10 minutes)

1. Open <https://console.cloud.google.com/> and create a project (for example "Notes").
2. **APIs & Services → Library**: enable **Google Calendar API**.
3. **Google Auth Platform → Branding**: app name "Notes", your support email.
4. **Audience**:
   - If everyone who will use it is in your own Google Workspace, choose **Internal**. That needs no Google review, and
     sign-ins don't expire.
   - Otherwise choose **External**, leave it in **Testing**, and add each tester's Google address under **Test users**.
     In Testing, Google expires the sign-in after 7 days, so testers reconnect weekly. Publishing it for everyone needs
     Google's verification of the calendar scope, which takes a few weeks.
5. **Data access → Add or remove scopes**: `openid`, `.../auth/userinfo.email`, `.../auth/userinfo.profile`,
   `.../auth/calendar.readonly`.
6. **Clients → Create client → Application type: Desktop app**, name "Notes for Mac". Copy the **Client ID** and the
   **Client secret**. Google says a desktop client's secret isn't confidential, but keep it out of the repo all the same.
7. In `apps/desktop/.env`, uncomment and fill in:
   ```
   NOTES_GOOGLE_CLIENT_ID=...
   NOTES_GOOGLE_CLIENT_SECRET=...
   ```

### Microsoft (about 10 minutes)

1. Open <https://entra.microsoft.com/> → **App registrations → New registration**.
2. Name "Notes for Mac". **Supported account types**: "Accounts in any organizational directory and personal Microsoft
   accounts" (or "this organizational directory only" to limit it to your company).
3. **Redirect URI**: platform **Public client/native (mobile & desktop)**, value `http://localhost`. No port; Microsoft
   accepts any port for `localhost`.
4. Register, then copy the **Application (client) ID**. Don't create a client secret: a desktop app is a public client.
5. **API permissions → Add → Microsoft Graph → Delegated**: `Calendars.Read`, `User.Read`, `offline_access`, `openid`,
   `profile`, `email`.
   - Users in your own organisation can consent themselves, unless your IT policy needs an admin.
   - Other organisations may need their admin to approve the app until it has a verified publisher.
6. In `apps/desktop/.env`, uncomment and fill in:
   ```
   NOTES_MICROSOFT_CLIENT_ID=...
   ```
   Optional: `NOTES_MICROSOFT_TENANT=<your tenant id>` limits sign-in to one organisation (default `common`).

### Try it

1. Restart the app (`.env` is read when it starts).
2. Open **Calendar** in the sidebar → **Google** (or **Outlook**) → **Connect Google Calendar**.
3. Approve in the browser. The tab should say "You're connected".
4. Back in Notes, the account's calendars appear. Tick the ones to follow.

To disconnect: right-click the account's calendar group → **Disconnect**. For Google this also revokes the app's access; for
Microsoft, remove it under <https://myapps.microsoft.com> if you want it gone on their side too.
