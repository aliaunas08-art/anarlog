use crate::SettingsPluginExt;

#[tauri::command]
#[specta::specta]
pub(crate) async fn global_base<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<String, String> {
    app.settings()
        .global_base()
        .map(|p| p.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn settings_path<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<String, String> {
    app.settings()
        .settings_path()
        .map(|p| p.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn vault_base<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<String, String> {
    app.settings()
        .vault_base()
        .map(|p| p.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn load<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
) -> Result<serde_json::Value, String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| denied.to_string())?;
    app.settings().load().await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn save<R: tauri::Runtime>(
    aspc_window: tauri::Window<R>,
    app: tauri::AppHandle<R>,
    settings: serde_json::Value,
) -> Result<(), String> {
    anlg_aspc::require(
        &app,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let before = app.settings().load().await.unwrap_or_default();
    let keys = changed_keys(&before, &settings);
    app.settings()
        .save(settings)
        .await
        .map_err(|e| e.to_string())?;
    if !keys.is_empty() {
        anlg_audit_log::record(
            &app,
            anlg_audit_log::NewEntry::new(
                anlg_audit_log::actions::SETTINGS_CHANGED,
                "settings",
                "settings.json",
            )
            .ids("keys", keys),
        );
    }
    Ok(())
}

/// D1.4: the names (`section.key`, two levels) whose values changed, never
/// the values.
fn changed_keys(before: &serde_json::Value, after: &serde_json::Value) -> Vec<String> {
    let empty = serde_json::Map::new();
    let before = before.as_object().unwrap_or(&empty);
    let after = after.as_object().unwrap_or(&empty);
    let mut keys = std::collections::BTreeSet::new();
    for name in before.keys().chain(after.keys()) {
        let (old, new) = (before.get(name), after.get(name));
        if old == new {
            continue;
        }
        match (
            old.and_then(|v| v.as_object()),
            new.and_then(|v| v.as_object()),
        ) {
            (Some(old), Some(new)) => {
                for inner in old.keys().chain(new.keys()) {
                    if old.get(inner) != new.get(inner) {
                        keys.insert(format!("{name}.{inner}"));
                    }
                }
            }
            _ => {
                keys.insert(name.clone());
            }
        }
    }
    keys.into_iter()
        .filter(|key| anlg_audit_log::is_identifier(key))
        .take(100)
        .collect()
}

#[cfg(test)]
mod audit_tests {
    #[test]
    fn changed_keys_names_only_what_changed() {
        let before = serde_json::json!({"general": {"a": 1, "b": 2}, "x": "old"});
        let after =
            serde_json::json!({"general": {"a": 1, "b": 3}, "x": "secret value", "y": true});
        assert_eq!(
            super::changed_keys(&before, &after),
            vec!["general.b".to_string(), "x".into(), "y".into()]
        );
    }
}
