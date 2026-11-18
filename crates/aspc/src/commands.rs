//! A2.1 command registry: every Tauri command the app and its plugins expose,
//! with the `(DataType, Action)` it needs or `Public` with a reason.
//!
//! - `plugin` is the directory name under `plugins/` (`"app"` for the
//!   desktop app's own commands in `apps/desktop/src-tauri/src/lib.rs`).
//! - `Guard::Enforced`: the command calls `aspc::require` first (lock,
//!   role, window).
//! - `Guard::LockExempt`: it calls `aspc::require_ignoring_lock` (role and
//!   window), for the reason given; the lock is enforced elsewhere or the
//!   command must keep running while the app is relocked.
//! - `Guard::Pending`: the requirement is recorded but the command does not
//!   call the guard yet (listed in docs/ASPC_AUDIT.md).
//!
//! `tests/command_registry.rs` fails if this table and the real command
//! sets differ, or if a command marked guarded does not call the guard.

use crate::{Action, DataType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// `Public` commands: no data, no guard.
    NotApplicable,
    Enforced,
    LockExempt(&'static str),
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Data(DataType, Action),
    Public { reason: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandEntry {
    pub plugin: &'static str,
    pub name: &'static str,
    pub access: Access,
    pub guard: Guard,
}

impl CommandEntry {
    pub fn is_guarded(&self) -> bool {
        matches!(self.guard, Guard::Enforced | Guard::LockExempt(_))
    }
}

pub fn find(plugin: &str, name: &str) -> Option<&'static CommandEntry> {
    COMMANDS
        .iter()
        .find(|e| e.plugin == plugin && e.name == name)
}

// Reasons shared by many `Public` entries.
const UI: &str = "window, tray, dock or UI state; no user data";
const DEVICE: &str = "audio devices, OS permissions or app detection; no stored user data";
const MODELS: &str = "local model files and servers; no user data";
const PURE: &str = "pure computation over the caller's own input; reads and stores nothing";
const LOCK_SCREEN: &str = "needed to show the lock screen and unlock";
const TELEMETRY: &str = "analytics metadata only (C4/D6 govern content); no user data returned";
const FLOW: &str = "cancels or controls an operation already authorized; returns no data";
const BUILD: &str = "build or environment facts; no user data";
const OPENER: &str = "hands a URL or path to the OS; paths are limited by A3.6";

const SQL_LOCK: &str = "lock enforced per statement by the webview SQL authorizer, which lets only the lock screen's settings reads through";
const SYNC_BACKGROUND: &str = "background cloud sync (key setup, device enrollment, token refresh) runs while the app is locked, which never pauses sync; role and window are still checked, and only the main window runs sync";
const CAPTURE_TOKEN: &str = "a timer refreshes the transcription token during an active recording, which keeps running while the app is locked; role and window are still checked";
const MEETING_BACKGROUND: &str = "meeting work that keeps running behind the lock screen after a relock (scheduled auto-start, enhance results, screen-record cataloguing); role and window are still checked, and only the main window runs it";
const TRANSCRIPT_PIPELINE: &str = "live, batch and dictation transcription keep running behind the lock screen after a relock and must save what they captured; role and window are still checked";
const CAPTURE_PIPELINE: &str = "recording, meeting detection, system dictation and the end-of-recording audio save and cleanup keep running behind the lock screen (a scheduled recording can start after a relock); role and window are still checked";
const CONTENT_BACKGROUND: &str = "background content work behind the lock screen after a relock: enhance reading meeting screenshots, screen records saved or removed as a capture ends, and cloud-sync evictions removing a session folder; role and window are still checked";
const CALENDAR_SYNC: &str = "periodic calendar sync keeps running behind the lock screen after a relock, so scheduled auto-start sees current events; role and window are still checked, and only the main window runs it";
const NOTES_BACKGROUND: &str = "background sync and capture work behind the lock screen after a relock: attachment sync and the shared-note attachment cache, enterprise session ingest, the meeting-chat recording disclosure, local API event dispatch, and tab state that sync-driven tab closes persist; role and window are still checked";
const AUTH_SESSION: &str =
    "background token refresh runs while relocked; denying it would sign the user out";

macro_rules! public {
    ($plugin:literal, $name:literal, $reason:expr) => {
        CommandEntry {
            plugin: $plugin,
            name: $name,
            access: Access::Public { reason: $reason },
            guard: Guard::NotApplicable,
        }
    };
}

macro_rules! pending {
    ($plugin:literal, $name:literal, $data:ident, $action:ident) => {
        CommandEntry {
            plugin: $plugin,
            name: $name,
            access: Access::Data(DataType::$data, Action::$action),
            guard: Guard::Pending,
        }
    };
}

macro_rules! guarded {
    ($plugin:literal, $name:literal, $data:ident, $action:ident) => {
        CommandEntry {
            plugin: $plugin,
            name: $name,
            access: Access::Data(DataType::$data, Action::$action),
            guard: Guard::Enforced,
        }
    };
}

macro_rules! lock_exempt {
    ($plugin:literal, $name:literal, $data:ident, $action:ident, $reason:expr) => {
        CommandEntry {
            plugin: $plugin,
            name: $name,
            access: Access::Data(DataType::$data, Action::$action),
            guard: Guard::LockExempt($reason),
        }
    };
}

pub static COMMANDS: &[CommandEntry] = &[
    // accounts: calendar sign-in accounts; tokens in the Keychain.
    public!("accounts", "cancel_connect", FLOW),
    public!("accounts", "configured_providers", BUILD),
    guarded!("accounts", "connect_account", Keys, Create),
    guarded!("accounts", "disconnect_account", Keys, Delete),
    pending!("accounts", "list_accounts", Settings, Read),
    // agent
    public!("agent", "health_check", BUILD),
    pending!("agent", "install_cli", Settings, Administer),
    pending!("agent", "uninstall_cli", Settings, Administer),
    // ai-engine: the Efficiency Engine (C7). The key never leaves Rust.
    public!("ai-engine", "cancel_request", FLOW),
    guarded!("ai-engine", "clear_api_key", Keys, Edit),
    guarded!("ai-engine", "key_status", Keys, Read),
    guarded!("ai-engine", "request", Notes, Read),
    guarded!("ai-engine", "set_api_key", Keys, Edit),
    guarded!("ai-engine", "set_quotas", Settings, Administer),
    guarded!("ai-engine", "status", Settings, Read),
    // audit: Settings -> Audit (D1.6). Reading the log is Settings/Administer;
    // the record_* commands append one fixed event shape each (ids only) and
    // there is no command that edits or deletes entries.
    guarded!("audit", "list_entries", Settings, Administer),
    guarded!("audit", "record_ask", Notes, Read),
    guarded!("audit", "record_meeting_view", Notes, Read),
    guarded!("audit", "record_settings_change", Settings, Edit),
    guarded!("audit", "verify", Settings, Administer),
    // analytics
    public!("analytics", "clear_groups", TELEMETRY),
    public!("analytics", "event", TELEMETRY),
    public!("analytics", "event_fire_and_forget", TELEMETRY),
    public!("analytics", "identify", TELEMETRY),
    public!("analytics", "is_disabled", TELEMETRY),
    pending!("analytics", "set_disabled", Settings, Edit),
    public!("analytics", "set_properties", TELEMETRY),
    // app (apps/desktop/src-tauri)
    public!("app", "check_embedded_cli", BUILD),
    public!("app", "complete_app_exit", FLOW),
    pending!("app", "get_account_full_name", Settings, Read),
    public!(
        "app",
        "get_data_protection_status",
        "whether FileVault is on (C2.1); a device fact, no user data"
    ),
    pending!("app", "get_dismissed_toasts", Settings, Read),
    public!(
        "app",
        "get_env",
        "allowlisted non-secret build flags only (A3.7)"
    ),
    // D4.2 "Where your data lives": local paths only.
    guarded!("app", "get_data_locations", Settings, Read),
    public!("app", "get_onboarding_needed", UI),
    guarded!("app", "get_pinned_tabs", Notes, Read),
    guarded!("app", "get_recently_opened_sessions", Notes, Read),
    pending!("app", "get_tinybase_values", Settings, Read),
    guarded!("app", "install_agent_skill", Settings, Administer),
    guarded!("app", "install_embedded_cli", Settings, Administer),
    public!("app", "is_app_store_build", BUILD),
    pending!("app", "is_crash_reporting_enabled", Settings, Read),
    public!("app", "list_skill_agents", BUILD),
    pending!("app", "set_crash_reporting_enabled", Settings, Edit),
    pending!("app", "set_dismissed_toasts", Settings, Edit),
    pending!("app", "set_onboarding_needed", Settings, Edit),
    lock_exempt!("app", "set_pinned_tabs", Notes, Edit, NOTES_BACKGROUND),
    lock_exempt!(
        "app",
        "set_recently_opened_sessions",
        Notes,
        Edit,
        NOTES_BACKGROUND
    ),
    public!("app", "show_devtool", UI),
    // attachment-sync
    lock_exempt!(
        "attachment-sync",
        "begin_attachment_download",
        Notes,
        Read,
        NOTES_BACKGROUND
    ),
    pending!(
        "attachment-sync",
        "begin_shared_upload_operation",
        Notes,
        Share
    ),
    public!("attachment-sync", "cancel_attachment_download", FLOW),
    public!("attachment-sync", "cancel_shared_upload_operation", FLOW),
    guarded!("attachment-sync", "cleanup_shared_upload", Notes, Share),
    lock_exempt!(
        "attachment-sync",
        "cleanup_transfer_cache",
        Notes,
        Edit,
        NOTES_BACKGROUND
    ),
    pending!(
        "attachment-sync",
        "clear_shared_attachment_preview_scopes",
        Notes,
        Edit
    ),
    pending!(
        "attachment-sync",
        "clear_shared_attachment_scope",
        Notes,
        Edit
    ),
    lock_exempt!(
        "attachment-sync",
        "commit_delete_guard",
        Notes,
        Delete,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "describe_upload",
        Notes,
        Read,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "download_and_restore",
        Notes,
        Create,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "download_shared_attachment",
        Notes,
        Read,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "prepare_delete_guard",
        Notes,
        Delete,
        NOTES_BACKGROUND
    ),
    guarded!("attachment-sync", "prepare_shared_upload", Notes, Share),
    lock_exempt!(
        "attachment-sync",
        "prepare_upload",
        Notes,
        Read,
        NOTES_BACKGROUND
    ),
    guarded!("attachment-sync", "read_shared_upload_range", Notes, Share),
    lock_exempt!(
        "attachment-sync",
        "read_upload_range",
        Notes,
        Read,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "reconcile_delete_guards",
        Notes,
        Delete,
        NOTES_BACKGROUND
    ),
    lock_exempt!(
        "attachment-sync",
        "remove_shared_attachment",
        Notes,
        Share,
        NOTES_BACKGROUND
    ),
    guarded!("attachment-sync", "shared_attachment_path", Notes, Read),
    guarded!("attachment-sync", "validate_shared_upload", Notes, Share),
    // audio-priority
    pending!("audio-priority", "get_input_priorities", Settings, Read),
    pending!("audio-priority", "get_output_priorities", Settings, Read),
    public!("audio-priority", "list_input_devices", DEVICE),
    public!("audio-priority", "list_output_devices", DEVICE),
    pending!("audio-priority", "save_input_priorities", Settings, Edit),
    pending!("audio-priority", "save_output_priorities", Settings, Edit),
    public!("audio-priority", "set_default_input_device", DEVICE),
    public!("audio-priority", "set_default_output_device", DEVICE),
    // auth: the hosted sign-in session, kept in the Keychain.
    public!(
        "auth",
        "begin_refresh",
        "coordinates refresh timing between windows; stores and returns no token"
    ),
    lock_exempt!("auth", "clear", Keys, Delete, AUTH_SESSION),
    public!("auth", "decode_claims", PURE),
    public!(
        "auth",
        "finish_refresh",
        "coordinates refresh timing between windows; stores and returns no token"
    ),
    pending!("auth", "get_account_info", Settings, Read),
    lock_exempt!("auth", "get_item", Keys, Read, AUTH_SESSION),
    lock_exempt!("auth", "remove_item", Keys, Delete, AUTH_SESSION),
    lock_exempt!("auth", "set_item", Keys, Edit, AUTH_SESSION),
    // bedrock
    public!("bedrock", "list_foundation_models", MODELS),
    // calendar: events and contacts are meeting metadata (Notes).
    lock_exempt!(
        "calendar",
        "apply_calendar_inventory",
        Notes,
        Edit,
        CALENDAR_SYNC
    ),
    guarded!("calendar", "apply_contact_enhancement", Notes, Edit),
    public!("calendar", "available_providers", BUILD),
    guarded!("calendar", "create_event", Notes, Create),
    guarded!("calendar", "create_human", Notes, Create),
    guarded!("calendar", "create_organization", Notes, Create),
    public!("calendar", "is_provider_enabled", DEVICE),
    lock_exempt!("calendar", "list_calendars", Notes, Read, CALENDAR_SYNC),
    lock_exempt!(
        "calendar",
        "list_connection_ids",
        Settings,
        Read,
        CALENDAR_SYNC
    ),
    lock_exempt!("calendar", "list_events", Notes, Read, CALENDAR_SYNC),
    guarded!("calendar", "merge_humans", Notes, Edit),
    public!("calendar", "open_calendar", OPENER),
    guarded!("calendar", "reorder_pinned_contacts", Notes, Edit),
    guarded!("calendar", "save_personal_contact", Notes, Edit),
    guarded!("calendar", "set_calendar_enabled", Settings, Edit),
    guarded!("calendar", "soft_delete_contact", Notes, Delete),
    lock_exempt!(
        "calendar",
        "sync_calendar_connection_events",
        Notes,
        Edit,
        CALENDAR_SYNC
    ),
    guarded!("calendar", "toggle_contact_pin", Notes, Edit),
    lock_exempt!(
        "calendar",
        "tombstone_calendar_connection",
        Notes,
        Delete,
        CALENDAR_SYNC
    ),
    guarded!("calendar", "update_contact_avatar", Notes, Edit),
    guarded!("calendar", "update_human", Notes, Edit),
    guarded!("calendar", "update_human_contact_summary", Notes, Edit),
    guarded!("calendar", "update_ignored_calendar_item", Settings, Edit),
    guarded!("calendar", "update_organization", Notes, Edit),
    // db
    lock_exempt!(
        "db",
        "apply_session_ingest",
        Notes,
        Create,
        NOTES_BACKGROUND
    ),
    pending!("db", "begin_cloudsync_activity", Settings, Edit),
    pending!("db", "bind_cloudsync_account", Settings, Administer),
    pending!("db", "cleanup_legacy_files", Settings, Administer),
    guarded!("db", "cleanup_recording_imports", Recording, Delete),
    pending!("db", "configure_cloudsync", Settings, Administer),
    lock_exempt!(
        "db",
        "configure_cloudsync_token",
        Keys,
        Edit,
        SYNC_BACKGROUND
    ),
    lock_exempt!("db", "configure_e2ee_replica", Keys, Edit, SYNC_BACKGROUND),
    pending!("db", "connect_local_library", Settings, Administer),
    lock_exempt!("db", "create_e2ee_identity", Keys, Create, SYNC_BACKGROUND),
    pending!("db", "end_cloudsync_activity", Settings, Edit),
    lock_exempt!("db", "execute", Notes, Read, SQL_LOCK),
    lock_exempt!("db", "execute_proxy", Notes, Read, SQL_LOCK),
    lock_exempt!("db", "execute_transaction", Notes, Edit, SQL_LOCK),
    pending!("db", "get_cloudsync_status", Settings, Read),
    lock_exempt!(
        "db",
        "get_e2ee_identity_status",
        Keys,
        Read,
        SYNC_BACKGROUND
    ),
    pending!("db", "get_legacy_cleanup_status", Settings, Read),
    pending!("db", "get_legacy_import_report", Settings, Read),
    guarded!("db", "get_meeting", Notes, Read),
    guarded!("db", "get_meeting_transcript", Transcript, Read),
    lock_exempt!(
        "db",
        "get_or_create_e2ee_device_identity",
        Keys,
        Create,
        SYNC_BACKGROUND
    ),
    guarded!("db", "get_recurring_meeting_history", Notes, Read),
    public!("db", "get_startup_status", LOCK_SCREEN),
    lock_exempt!(
        "db",
        "import_e2ee_device_enrollment",
        Keys,
        Create,
        SYNC_BACKGROUND
    ),
    lock_exempt!("db", "import_e2ee_identity", Keys, Create, SYNC_BACKGROUND),
    lock_exempt!(
        "db",
        "inspect_e2ee_recovery_key",
        Keys,
        Read,
        SYNC_BACKGROUND
    ),
    guarded!("db", "list_meetings", Notes, Read),
    guarded!("db", "run_legacy_import", Notes, Create),
    lock_exempt!(
        "db",
        "seal_e2ee_recovery_key_for_device",
        Keys,
        Share,
        SYNC_BACKGROUND
    ),
    lock_exempt!(
        "db",
        "seal_workspace_e2ee_key_for_recipients",
        Keys,
        Share,
        SYNC_BACKGROUND
    ),
    pending!("db", "start_cloudsync", Settings, Edit),
    pending!("db", "stop_cloudsync", Settings, Edit),
    lock_exempt!("db", "subscribe", Notes, Read, SQL_LOCK),
    pending!("db", "suspend_cloudsync", Settings, Edit),
    pending!("db", "suspend_cloudsync_after_auth_loss", Settings, Edit),
    pending!("db", "suspend_cloudsync_for_sign_out", Settings, Edit),
    pending!("db", "sync_cloudsync_now", Settings, Edit),
    public!("db", "unsubscribe", FLOW),
    public!("db", "wait_until_ready", LOCK_SCREEN),
    // deeplink2
    public!(
        "deeplink2",
        "list_pending_share_opens",
        "pending deep-link payloads the OS delivered; no stored data"
    ),
    public!("deeplink2", "start_callback_server", FLOW),
    public!("deeplink2", "stop_callback_server", FLOW),
    public!(
        "deeplink2",
        "take_pending_deep_links",
        "pending deep-link payloads the OS delivered; no stored data"
    ),
    public!(
        "deeplink2",
        "take_pending_share_open",
        "pending deep-link payloads the OS delivered; no stored data"
    ),
    // detect
    lock_exempt!(
        "detect",
        "capture_meeting_chat_messages",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "detect",
        "capture_meeting_screen_share",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    public!("detect", "get_current_locale_identifier", DEVICE),
    public!("detect", "get_installed_application_icons", DEVICE),
    public!("detect", "get_preferred_languages", DEVICE),
    lock_exempt!(
        "detect",
        "inspect_meeting_accessibility",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    public!("detect", "list_default_ignored_bundle_ids", DEVICE),
    public!("detect", "list_installed_applications", DEVICE),
    public!("detect", "list_mic_using_applications", DEVICE),
    lock_exempt!(
        "detect",
        "send_meeting_chat_message",
        Notes,
        Share,
        NOTES_BACKGROUND
    ),
    pending!("detect", "set_ignored_bundle_ids", Settings, Edit),
    pending!("detect", "set_included_bundle_ids", Settings, Edit),
    pending!("detect", "set_mic_active_threshold", Settings, Edit),
    pending!("detect", "set_respect_do_not_disturb", Settings, Edit),
    lock_exempt!(
        "detect",
        "start_active_speaker_tracking",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "detect",
        "stop_active_speaker_tracking",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    // dictation
    public!("dictation", "cancel_recording", FLOW),
    public!("dictation", "capture_target", DEVICE),
    public!("dictation", "discard_recording", FLOW),
    public!("dictation", "hide", UI),
    public!(
        "dictation",
        "insert_text",
        "types the caller's text into the focused app; stores nothing"
    ),
    public!("dictation", "set_phase", UI),
    public!("dictation", "show", UI),
    guarded!("dictation", "start_recording", Recording, Create),
    lock_exempt!(
        "dictation",
        "start_system_recording",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "dictation",
        "stop_recording",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    public!("dictation", "update_amplitude", UI),
    // export
    guarded!("export", "export", Notes, Export),
    // fs-sync: the vault mirror of meetings on disk.
    lock_exempt!(
        "fs-sync",
        "attachment_list",
        Notes,
        Read,
        CONTENT_BACKGROUND
    ),
    lock_exempt!(
        "fs-sync",
        "attachment_read",
        Notes,
        Read,
        CONTENT_BACKGROUND
    ),
    lock_exempt!(
        "fs-sync",
        "attachment_remove",
        Notes,
        Delete,
        CONTENT_BACKGROUND
    ),
    lock_exempt!(
        "fs-sync",
        "attachment_save",
        Notes,
        Create,
        CONTENT_BACKGROUND
    ),
    guarded!("fs-sync", "audio_copy", Recording, Create),
    lock_exempt!(
        "fs-sync",
        "audio_delete",
        Recording,
        Delete,
        CAPTURE_PIPELINE
    ),
    pending!(
        "fs-sync",
        "audio_delete_orphaned_expired",
        Recording,
        Delete
    ),
    lock_exempt!("fs-sync", "audio_exist", Recording, Read, CAPTURE_PIPELINE),
    lock_exempt!(
        "fs-sync",
        "audio_has_speech",
        Recording,
        Read,
        CAPTURE_PIPELINE
    ),
    guarded!("fs-sync", "audio_import", Recording, Create),
    guarded!("fs-sync", "audio_import_data", Recording, Create),
    lock_exempt!(
        "fs-sync",
        "audio_metadata",
        Recording,
        Read,
        CAPTURE_PIPELINE
    ),
    lock_exempt!("fs-sync", "audio_path", Recording, Read, CAPTURE_PIPELINE),
    guarded!("fs-sync", "audio_peaks", Recording, Read),
    lock_exempt!(
        "fs-sync",
        "audio_source_metadata",
        Recording,
        Read,
        CAPTURE_PIPELINE
    ),
    guarded!("fs-sync", "chat_dir", Notes, Read),
    guarded!("fs-sync", "create_folder", Notes, Create),
    guarded!("fs-sync", "delete_folder", Notes, Delete),
    lock_exempt!(
        "fs-sync",
        "delete_session_folder",
        Notes,
        Delete,
        CONTENT_BACKGROUND
    ),
    public!("fs-sync", "deserialize", PURE),
    guarded!("fs-sync", "entity_dir", Notes, Read),
    guarded!("fs-sync", "folder_attachment_list", Notes, Read),
    guarded!("fs-sync", "folder_attachment_read", Notes, Read),
    guarded!("fs-sync", "folder_attachment_remove", Notes, Delete),
    guarded!("fs-sync", "folder_attachment_save", Notes, Create),
    guarded!("fs-sync", "list_folders", Notes, Read),
    guarded!("fs-sync", "load_session_content", Notes, Read),
    guarded!("fs-sync", "move_session", Notes, Edit),
    guarded!("fs-sync", "read_document_batch", Notes, Read),
    guarded!("fs-sync", "rename_folder", Notes, Edit),
    guarded!("fs-sync", "scan_and_read", Notes, Read),
    guarded!("fs-sync", "session_dir", Notes, Read),
    guarded!("fs-sync", "write_document_batch", Notes, Edit),
    guarded!("fs-sync", "write_json_batch", Notes, Edit),
    // fs2
    public!(
        "fs2",
        "pick_open_paths",
        "native dialog; the user picks the path in Rust (A3.6)"
    ),
    public!(
        "fs2",
        "pick_save_path",
        "native dialog; the user picks the path in Rust (A3.6)"
    ),
    guarded!("fs2", "read_text_file", Notes, Read),
    guarded!("fs2", "remove", Notes, Delete),
    guarded!("fs2", "write_text_file", Notes, Export),
    // icon
    public!("icon", "get_icon", UI),
    public!("icon", "reset_dock_icon", UI),
    public!("icon", "set_dock_icon", UI),
    public!("icon", "set_notification_badge", UI),
    public!("icon", "set_recording_indicator", UI),
    // importer
    guarded!("importer", "begin_connected_import", Notes, Create),
    public!("importer", "cancel_connected_import", FLOW),
    guarded!("importer", "complete_connected_import", Notes, Create),
    guarded!("importer", "read_text_files", Notes, Create),
    guarded!("importer", "sync_connected_import", Notes, Create),
    // js
    public!(
        "js",
        "eval",
        "sandboxed QuickJS over the caller's code; no app or file access"
    ),
    // local-api
    guarded!("local-api", "create_webhook", Keys, Create),
    guarded!("local-api", "delete_webhook", Settings, Delete),
    lock_exempt!(
        "local-api",
        "dispatch_event",
        Notes,
        Share,
        NOTES_BACKGROUND
    ),
    guarded!("local-api", "export_meeting_markdown", Notes, Export),
    guarded!("local-api", "get_cloud_snapshot", Notes, Read),
    guarded!("local-api", "list_cloud_snapshot_ids", Notes, Read),
    guarded!("local-api", "list_webhooks", Settings, Read),
    guarded!("local-api", "prepare_drive_markdown", Notes, Export),
    guarded!("local-api", "set_webhook_active", Settings, Edit),
    guarded!("local-api", "test_webhook", Notes, Share),
    // local-auth: device authentication and the Rust-held app lock.
    public!(
        "local-auth",
        "access_context",
        "non-secret ids of the local access context"
    ),
    public!("local-auth", "app_lock_state", LOCK_SCREEN),
    public!("local-auth", "authenticate", LOCK_SCREEN),
    public!("local-auth", "available", LOCK_SCREEN),
    public!("local-auth", "lock_app", "locking only tightens access"),
    guarded!("local-auth", "set_app_lock_enabled", Settings, Administer),
    public!("local-auth", "unlock_app", LOCK_SCREEN),
    // local-llm
    public!("local-llm", "cancel_download", MODELS),
    public!("local-llm", "delete_model", MODELS),
    public!("local-llm", "download_model", MODELS),
    public!("local-llm", "foundation_model_availability", MODELS),
    public!("local-llm", "foundation_model_begin", MODELS),
    public!("local-llm", "foundation_model_cancel", MODELS),
    public!(
        "local-llm",
        "foundation_model_generate",
        "on-device model over the caller's text; stores nothing"
    ),
    public!("local-llm", "is_model_downloaded", MODELS),
    public!("local-llm", "is_model_downloading", MODELS),
    public!("local-llm", "list_custom_models", MODELS),
    public!("local-llm", "list_downloaded_model", MODELS),
    public!("local-llm", "list_supported_model", MODELS),
    public!("local-llm", "models_dir", MODELS),
    public!("local-llm", "server_url", MODELS),
    // local-stt
    public!("local-stt", "cancel_download", MODELS),
    public!("local-stt", "delete_model", MODELS),
    public!("local-stt", "download_model", MODELS),
    public!("local-stt", "get_server_for_model", MODELS),
    public!("local-stt", "get_servers", MODELS),
    public!("local-stt", "inspect_custom_model_path", MODELS),
    public!("local-stt", "is_model_downloaded", MODELS),
    public!("local-stt", "is_model_downloading", MODELS),
    public!("local-stt", "list_supported_models", MODELS),
    public!("local-stt", "models_dir", MODELS),
    public!("local-stt", "soniqo_model_dir", MODELS),
    public!("local-stt", "start_server", MODELS),
    public!("local-stt", "start_server_for_path", MODELS),
    public!("local-stt", "stop_server", MODELS),
    // messenger
    public!("messenger", "ping", BUILD),
    // misc
    pending!("misc", "allow_http_origin", Settings, Administer),
    public!("misc", "get_device_info", BUILD),
    public!("misc", "get_fingerprint", BUILD),
    public!("misc", "get_git_hash", BUILD),
    public!("misc", "get_process_memory_bytes", BUILD),
    public!("misc", "opinionated_md_to_html", PURE),
    // notification
    public!("notification", "clear_notifications", UI),
    public!("notification", "show_notification", UI),
    // opener2
    public!("opener2", "open_path", OPENER),
    public!("opener2", "open_url", OPENER),
    public!("opener2", "reveal_item_in_dir", OPENER),
    // overlay
    public!("overlay", "remove_fake_window", UI),
    public!("overlay", "set_fake_window_bounds", UI),
    // path2
    public!("path2", "sanitize", PURE),
    // permissions
    public!("permissions", "check_permission", DEVICE),
    public!("permissions", "close_permission_assistant", DEVICE),
    public!("permissions", "open_permission", DEVICE),
    public!("permissions", "permission_guidance", DEVICE),
    public!("permissions", "request_permission", DEVICE),
    public!("permissions", "reset_permission", DEVICE),
    // relay (debug and devtools builds only)
    public!(
        "relay",
        "relay_result",
        "dev-only automation relay; not built into release"
    ),
    // session
    guarded!("session", "add_session_participant", Notes, Edit),
    guarded!("session", "catalog_folder_material", Notes, Create),
    lock_exempt!(
        "session",
        "catalog_note_attachment",
        Notes,
        Create,
        MEETING_BACKGROUND
    ),
    lock_exempt!(
        "session",
        "catalog_session_audio",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "session",
        "create_session",
        Notes,
        Create,
        MEETING_BACKGROUND
    ),
    lock_exempt!(
        "session",
        "create_session_for_event",
        Notes,
        Create,
        MEETING_BACKGROUND
    ),
    guarded!("session", "delete_folder_catalog", Notes, Delete),
    lock_exempt!(
        "session",
        "ensure_folder_catalog",
        Notes,
        Create,
        MEETING_BACKGROUND
    ),
    lock_exempt!(
        "session",
        "mark_session_audio_absent",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    pending!(
        "session",
        "mark_session_audio_transcription_complete",
        Transcript,
        Edit
    ),
    guarded!("session", "move_session_contents", Notes, Edit),
    guarded!("session", "persist_chat_session_proposal", Notes, Create),
    guarded!("session", "remove_session_participant", Notes, Edit),
    guarded!("session", "rename_folder_catalog", Notes, Edit),
    guarded!("session", "resolve_session_conflict", Notes, Edit),
    guarded!("session", "resolve_session_conflicts", Notes, Edit),
    // D2.2 permanent delete after the undo window (soft-deleted meetings only).
    guarded!("session", "purge_deleted_session", Notes, Delete),
    // C5 backup (the whole library leaves the app: Export) and restore
    // (replaces the library on the next launch: Administer). Main window
    // only (capabilities/main.json); both confirm in a native dialog.
    guarded!("session", "backup_library", Notes, Export),
    guarded!("session", "restore_library", Settings, Administer),
    guarded!("session", "restore_deleted_session", Notes, Edit),
    pending!(
        "session",
        "set_attachment_cloud_sync_enabled",
        Settings,
        Edit
    ),
    guarded!("session", "set_session_proposal_status", Notes, Edit),
    guarded!("session", "soft_delete_session", Notes, Delete),
    guarded!("session", "tombstone_folder_material", Notes, Delete),
    guarded!("session", "tombstone_session_audio", Recording, Delete),
    guarded!("session", "update_folder_icon", Notes, Edit),
    guarded!("session", "update_folder_instructions", Notes, Edit),
    guarded!("session", "update_folder_workspace", Notes, Edit),
    // settings
    public!("settings", "global_base", BUILD),
    pending!("settings", "load", Settings, Read),
    guarded!("settings", "save", Settings, Edit),
    public!("settings", "settings_path", BUILD),
    public!("settings", "vault_base", BUILD),
    // sfx
    public!("sfx", "play", UI),
    public!("sfx", "set_volume", UI),
    public!("sfx", "stop", UI),
    // shortcut
    public!("shortcut", "configure", UI),
    public!("shortcut", "register_hotkey", UI),
    public!("shortcut", "set_active", UI),
    public!("shortcut", "unregister_hotkey", UI),
    public!("shortcut", "validate", UI),
    // store2: plain key-value store and Keychain secrets.
    guarded!("store2", "delete_secret", Keys, Delete),
    pending!("store2", "get_bool", Settings, Read),
    pending!("store2", "get_number", Settings, Read),
    guarded!("store2", "get_secret", Keys, Read),
    pending!("store2", "get_str", Settings, Read),
    guarded!("store2", "repair_keychain_access", Keys, Administer),
    pending!("store2", "save", Settings, Edit),
    pending!("store2", "set_bool", Settings, Edit),
    pending!("store2", "set_number", Settings, Edit),
    guarded!("store2", "set_secret", Keys, Edit),
    pending!("store2", "set_str", Settings, Edit),
    // tantivy
    guarded!("tantivy", "add_document", Notes, Edit),
    guarded!("tantivy", "reindex", Notes, Edit),
    guarded!("tantivy", "remove_document", Notes, Edit),
    guarded!("tantivy", "search", Notes, Read),
    guarded!("tantivy", "update_document", Notes, Edit),
    guarded!("tantivy", "update_documents", Notes, Edit),
    // template
    guarded!("template", "apply_session_content_corrections", Notes, Edit),
    public!("template", "compose_generated_summary", PURE),
    public!("template", "dominant_language", PURE),
    pending!("template", "get_template_source", Settings, Read),
    public!("template", "prepare_generated_summary", PURE),
    public!("template", "render", PURE),
    public!("template", "render_custom", PURE),
    lock_exempt!(
        "template",
        "save_generated_summary",
        Notes,
        Edit,
        MEETING_BACKGROUND
    ),
    lock_exempt!(
        "template",
        "save_generated_title",
        Notes,
        Edit,
        MEETING_BACKGROUND
    ),
    public!("template", "summary_length_policy", PURE),
    // todo
    public!("todo", "authorization_status", DEVICE),
    pending!("todo", "complete_todo", Actions, Edit),
    pending!("todo", "create_todo", Actions, Create),
    pending!("todo", "delete_todo", Actions, Delete),
    pending!("todo", "fetch_todos", Actions, Read),
    pending!("todo", "github_issue_comments", Actions, Read),
    pending!("todo", "github_issue_detail", Actions, Read),
    pending!("todo", "github_issue_state", Actions, Read),
    pending!("todo", "linear_list_teams", Actions, Read),
    pending!("todo", "linear_list_tickets", Actions, Read),
    pending!("todo", "list_todo_lists", Actions, Read),
    pending!("todo", "read_path", Actions, Read),
    public!("todo", "request_full_access", DEVICE),
    // tracing
    public!(
        "tracing",
        "do_log",
        "writes a log line; C4 redaction applies"
    ),
    pending!("tracing", "log_content", Settings, Read),
    public!("tracing", "logs_dir", BUILD),
    // transcription
    pending!(
        "transcription",
        "acknowledge_capture_audio_chunk",
        Recording,
        Edit
    ),
    pending!(
        "transcription",
        "acknowledge_capture_audio_cleanup_status",
        Recording,
        Edit
    ),
    pending!(
        "transcription",
        "acknowledge_completed_transcription",
        Transcript,
        Edit
    ),
    pending!(
        "transcription",
        "acknowledge_stopped_capture",
        Recording,
        Edit
    ),
    pending!(
        "transcription",
        "attribute_session_speakers",
        Transcript,
        Edit
    ),
    public!("transcription", "cancel_voice_enrollment", FLOW),
    pending!(
        "transcription",
        "clear_capture_audio_saved",
        Recording,
        Edit
    ),
    pending!(
        "transcription",
        "clear_capture_lifecycle_marker",
        Recording,
        Edit
    ),
    pending!(
        "transcription",
        "delete_processed_session_audio",
        Recording,
        Delete
    ),
    pending!(
        "transcription",
        "delete_transcribed_capture_audio",
        Recording,
        Delete
    ),
    guarded!(
        "transcription",
        "delete_voice_enrollment",
        Recording,
        Delete
    ),
    guarded!("transcription", "export_to_vtt", Transcript, Export),
    pending!(
        "transcription",
        "extract_voiceprint_candidates",
        Transcript,
        Read
    ),
    pending!(
        "transcription",
        "finish_voice_enrollment",
        Recording,
        Create
    ),
    lock_exempt!(
        "transcription",
        "flush_live_transcript",
        Transcript,
        Edit,
        TRANSCRIPT_PIPELINE
    ),
    pending!(
        "transcription",
        "get_capture_audio_cleanup_status",
        Recording,
        Read
    ),
    lock_exempt!(
        "transcription",
        "get_capture_audio_gaps",
        Recording,
        Read,
        CAPTURE_PIPELINE
    ),
    pending!(
        "transcription",
        "get_capture_lifecycle_marker",
        Recording,
        Read
    ),
    lock_exempt!(
        "transcription",
        "get_capture_snapshot",
        Transcript,
        Read,
        TRANSCRIPT_PIPELINE
    ),
    public!(
        "transcription",
        "get_capture_state",
        "capture state flag only (active, finalizing, inactive)"
    ),
    pending!(
        "transcription",
        "get_completed_transcription",
        Transcript,
        Read
    ),
    public!("transcription", "get_current_microphone_device", DEVICE),
    public!("transcription", "get_mic_muted", DEVICE),
    lock_exempt!(
        "transcription",
        "get_stopped_capture",
        Recording,
        Read,
        CAPTURE_PIPELINE
    ),
    pending!(
        "transcription",
        "improve_voice_enrollment_from_segment",
        Recording,
        Edit
    ),
    public!("transcription", "is_supported_languages_batch", BUILD),
    public!("transcription", "is_supported_languages_live", BUILD),
    pending!(
        "transcription",
        "list_capture_audio_chunks",
        Recording,
        Read
    ),
    pending!(
        "transcription",
        "list_capture_lifecycle_markers",
        Recording,
        Read
    ),
    guarded!("transcription", "list_capture_recoveries", Recording, Read),
    public!(
        "transcription",
        "list_documented_language_codes_batch",
        BUILD
    ),
    public!(
        "transcription",
        "list_documented_language_codes_live",
        BUILD
    ),
    public!("transcription", "list_microphone_devices", DEVICE),
    guarded!("transcription", "list_stopped_captures", Recording, Read),
    pending!(
        "transcription",
        "list_transcription_sessions",
        Transcript,
        Read
    ),
    lock_exempt!(
        "transcription",
        "mark_capture_audio_saved",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    public!("transcription", "parse_subtitle", PURE),
    pending!(
        "transcription",
        "promote_voiceprint_candidates",
        Transcript,
        Edit
    ),
    pending!(
        "transcription",
        "reconcile_refined_speaker_clusters",
        Transcript,
        Edit
    ),
    public!("transcription", "refine_batch_transcript", PURE),
    lock_exempt!(
        "transcription",
        "release_live_transcript",
        Transcript,
        Edit,
        TRANSCRIPT_PIPELINE
    ),
    guarded!(
        "transcription",
        "render_session_transcript",
        Transcript,
        Read
    ),
    public!("transcription", "render_transcript_segments", PURE),
    lock_exempt!(
        "transcription",
        "retry_audio_capture",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "transcription",
        "save_batch_transcript",
        Transcript,
        Create,
        TRANSCRIPT_PIPELINE
    ),
    pending!(
        "transcription",
        "save_capture_lifecycle_marker",
        Recording,
        Edit
    ),
    lock_exempt!(
        "transcription",
        "set_capture_paused",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    public!("transcription", "set_mic_muted", DEVICE),
    lock_exempt!(
        "transcription",
        "start_capture",
        Recording,
        Create,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "transcription",
        "start_transcription",
        Transcript,
        Create,
        TRANSCRIPT_PIPELINE
    ),
    guarded!("transcription", "start_voice_enrollment", Recording, Create),
    lock_exempt!(
        "transcription",
        "stop_capture",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "transcription",
        "stop_capture_for_session",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "transcription",
        "stop_transcription",
        Transcript,
        Edit,
        TRANSCRIPT_PIPELINE
    ),
    public!(
        "transcription",
        "suggest_providers_for_languages_batch",
        BUILD
    ),
    public!(
        "transcription",
        "suggest_providers_for_languages_live",
        BUILD
    ),
    lock_exempt!(
        "transcription",
        "update_capture_config",
        Recording,
        Edit,
        CAPTURE_PIPELINE
    ),
    lock_exempt!(
        "transcription",
        "update_capture_credentials",
        Keys,
        Edit,
        CAPTURE_TOKEN
    ),
    guarded!("transcription", "voice_enrollment_status", Recording, Read),
    // tray
    public!("tray", "set_tray_icon_visible", UI),
    public!("tray", "set_tray_recording_title", UI),
    public!("tray", "set_tray_schedule", UI),
    // updater2
    public!("updater2", "check", BUILD),
    public!("updater2", "download", BUILD),
    public!("updater2", "install_and_relaunch", BUILD),
    public!("updater2", "is_downloaded", BUILD),
    public!("updater2", "maybe_emit_updated", BUILD),
    pending!("updater2", "set_automatic_updates_enabled", Settings, Edit),
    public!("updater2", "set_meeting_active", UI),
    // windows
    public!("windows", "floating_bar_current_state", UI),
    public!("windows", "floating_bar_hide", UI),
    public!("windows", "floating_bar_show", UI),
    public!("windows", "floating_bar_update", UI),
    public!("windows", "floating_bar_update_amplitude", UI),
    public!("windows", "live_caption_current_state", UI),
    public!("windows", "live_caption_hide", UI),
    public!("windows", "live_caption_show", UI),
    public!("windows", "live_caption_update", UI),
    public!("windows", "set_show_app_in_dock", UI),
    public!("windows", "webview_health_ack", UI),
    public!("windows", "webview_health_ready", UI),
    public!("windows", "window_destroy", UI),
    public!("windows", "window_emit_navigate", UI),
    public!("windows", "window_expand_width", UI),
    public!("windows", "window_hide", UI),
    public!("windows", "window_is_exists", UI),
    public!("windows", "window_is_occluded", UI),
    public!("windows", "window_navigate", UI),
    public!("windows", "window_restore_frame_animated", UI),
    public!("windows", "window_restore_width", UI),
    public!("windows", "window_save_frame", UI),
    public!("windows", "window_set_frame_animated", UI),
    public!("windows", "window_show", UI),
];
