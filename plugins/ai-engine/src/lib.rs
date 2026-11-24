//! Tauri side of the Efficiency Engine (ASPC C7). Every AI call from the
//! webview goes through `ai-engine:request`; the key never leaves Rust.

mod commands;

use std::sync::Arc;

use anlg_ai_engine::Engine;
use anlg_ai_engine::config::KEYCHAIN_SERVICE_SUFFIX;
use anlg_local_accounts::KeychainStore;
use tauri::Manager;

const PLUGIN_NAME: &str = "ai-engine";

fn make_specta_builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
    tauri_specta::Builder::<R>::new()
        .plugin_name(PLUGIN_NAME)
        .commands(tauri_specta::collect_commands![
            commands::request::<tauri::Wry>,
            commands::cancel_request::<tauri::Wry>,
            commands::status::<tauri::Wry>,
            commands::key_status::<tauri::Wry>,
            commands::set_api_key::<tauri::Wry>,
            commands::clear_api_key::<tauri::Wry>,
            commands::set_quotas::<tauri::Wry>,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let specta_builder = make_specta_builder();

    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(|app, _| {
            let service = format!("{}.{KEYCHAIN_SERVICE_SUFFIX}", app.config().identifier);
            let data_dir = app
                .path()
                .app_data_dir()
                .ok()
                .map(|dir| dir.join(PLUGIN_NAME));
            let engine = Engine::new(
                anlg_ai_engine::resolve_engine_config(),
                Arc::new(KeychainStore::new(service)),
                data_dir,
            );
            app.manage(Arc::new(engine));
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn export_types() {
        const OUTPUT_FILE: &str = "./js/bindings.gen.ts";

        make_specta_builder::<tauri::Wry>()
            .export(
                specta_typescript::Typescript::default()
                    .formatter(specta_typescript::formatter::prettier)
                    .bigint(specta_typescript::BigIntExportBehavior::Number),
                OUTPUT_FILE,
            )
            .unwrap();

        let content = std::fs::read_to_string(OUTPUT_FILE).unwrap();
        std::fs::write(OUTPUT_FILE, format!("// @ts-nocheck\n{content}")).unwrap();
    }

    #[test]
    fn plugin_loads() {
        let _app = tauri::test::mock_builder()
            .plugin(init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
    }
}
