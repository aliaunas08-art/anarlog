use tauri_plugin_store2::ScopedStoreKey;

#[derive(serde::Deserialize, specta::Type, PartialEq, Eq, Hash, strum::Display)]
pub enum StoreKey {
    /// Pre-ASPC opt-out flag. Ignored: analytics was on unless this was set,
    /// so it can't show that a user opted in (ASPC D6, C6 defaults).
    Disabled,
    /// Set to `true` only when the user turns analytics on in Settings.
    OptedIn,
    LegacyDistinctIdAliased,
}

impl ScopedStoreKey for StoreKey {}
