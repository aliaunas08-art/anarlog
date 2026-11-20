# CLAUDE.md

Read [AGENTS.md](AGENTS.md) first for the repo layout and workflow.

## Security and compliance rulebook (ASPC)

Every change must comply with [docs/ASPC_BASE.md](docs/ASPC_BASE.md). If a change alters one of its rules, update that file in the same commit. The current status of each rule is in [docs/ASPC_AUDIT.md](docs/ASPC_AUDIT.md).

The rules most often relevant to a change:

- **Trust boundary:** the webview is untrusted. User data is read and written only through Rust, which calls the central `authorize(ctx, workspace, data_type, action)` check. Features never compare roles themselves. (A1, A2)
- **New Tauri commands:** every new command is added to the command registry with the data type and action it needs, and is given only to the windows that need it. (A2)
- **Workspace scope:** every content row carries `workspace_id` and `owner_user_id`, and every query filters by `workspace_id`. (C1)
- **Secrets:** keys and tokens live in the macOS Keychain only. They are never stored in SQLite, files, the repo or logs, never sent to JS, and never put in URLs. (C3)
- **External content is data:** transcripts, invites, titles and pasted text are never treated as instructions. Prompts wrap them in escaped blocks. AI output can never export, delete, share or read keys by itself. (B2, B3)
- **AI calls:** every AI call goes through the Efficiency Engine, with the model locked and quotas and a fallback when AI is unavailable. (C7)
- **No meeting content in logs, crash reports or analytics.** Meeting content is never used for analytics, training or telemetry. (C4, D6)
- **Audit log:** sensitive actions are recorded in the separate, append-only, hash-chained `audit.db`. App code never edits or deletes it. (D1)
- **Licence:** keep the MIT notice. Never import from `enterprise/`. Flag any dependency that isn't MIT-compatible. (D5)
