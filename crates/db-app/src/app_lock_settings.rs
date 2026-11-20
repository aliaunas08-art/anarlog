//! The app lock settings in `app_settings` (ASPC A4.2, A4.4), read the same
//! way by the desktop app and by the CLI and MCP server, which have no Rust
//! lock state of their own.

use sqlx::SqlitePool;

pub const LOCK_SETTING_ID: &str = "lock_app";
/// Minutes of no keyboard or mouse input before the app relocks (A4.4).
/// 0 turns the idle relock off.
pub const IDLE_SETTING_ID: &str = "lock_idle_minutes";
pub const DEFAULT_IDLE_MINUTES: u32 = 15;
/// The choices the settings screen offers; anything else is refused.
pub const IDLE_MINUTE_CHOICES: &[u32] = &[0, 1, 5, 15, 30, 60];
const LEGACY_SETTINGS_ID: &str = "legacy_settings_document";

fn parse_bool(value_json: &str) -> Option<bool> {
    match serde_json::from_str::<serde_json::Value>(value_json).ok()? {
        serde_json::Value::Bool(value) => Some(value),
        _ => None,
    }
}

/// Reads the `lock_app` setting the way the frontend does: the direct
/// `app_settings` row, else the legacy settings document's
/// `general.lock_app`, else off.
pub async fn read_lock_setting(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, value_json FROM app_settings WHERE id IN (?, ?)")
            .bind(LOCK_SETTING_ID)
            .bind(LEGACY_SETTINGS_ID)
            .fetch_all(pool)
            .await?;
    let direct = rows
        .iter()
        .find(|(id, _)| id == LOCK_SETTING_ID)
        .and_then(|(_, value)| parse_bool(value));
    if let Some(value) = direct {
        return Ok(value);
    }
    let legacy = rows
        .iter()
        .find(|(id, _)| id == LEGACY_SETTINGS_ID)
        .and_then(|(_, value)| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|doc| doc.get("general")?.get(LOCK_SETTING_ID)?.as_bool());
    Ok(legacy.unwrap_or(false))
}

pub async fn write_lock_setting(pool: &SqlitePool, enabled: bool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_settings (id, value_json) VALUES (?, ?)
         ON CONFLICT(id) DO UPDATE SET
           value_json = excluded.value_json,
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(LOCK_SETTING_ID)
    .bind(if enabled { "true" } else { "false" })
    .execute(pool)
    .await?;
    Ok(())
}

/// Reads the idle relock setting. A missing or unknown value means the
/// default, so a broken row can't turn the idle relock off.
pub async fn read_idle_setting(pool: &SqlitePool) -> Result<u32, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value_json FROM app_settings WHERE id = ?")
        .bind(IDLE_SETTING_ID)
        .fetch_optional(pool)
        .await?;
    Ok(row
        .and_then(|(value,)| serde_json::from_str::<u32>(&value).ok())
        .filter(|minutes| IDLE_MINUTE_CHOICES.contains(minutes))
        .unwrap_or(DEFAULT_IDLE_MINUTES))
}

pub async fn write_idle_setting(pool: &SqlitePool, minutes: u32) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO app_settings (id, value_json) VALUES (?, ?)
         ON CONFLICT(id) DO UPDATE SET
           value_json = excluded.value_json,
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(IDLE_SETTING_ID)
    .bind(minutes.to_string())
    .execute(pool)
    .await?;
    Ok(())
}
