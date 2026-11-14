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
