pub mod access;
mod app_lock;
mod commands;
mod error;
mod ext;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub use app_lock::{AppLockChanged, AppLockExt, RELOCK_GRACE};
pub use error::{Error, Result};
pub use ext::*;

const PLUGIN_NAME: &str = "local-auth";

fn make_specta_builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
    tauri_specta::Builder::<R>::new()
        .plugin_name(PLUGIN_NAME)
        .commands(tauri_specta::collect_commands![
            commands::available::<tauri::Wry>,
            commands::authenticate::<tauri::Wry>,
            commands::access_context::<tauri::Wry>,
            commands::app_lock_state::<tauri::Wry>,
            commands::unlock_app::<tauri::Wry>,
            commands::lock_app::<tauri::Wry>,
            commands::set_app_lock_enabled::<tauri::Wry>,
        ])
        .events(tauri_specta::collect_events![app_lock::AppLockChanged])
        .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let specta_builder = make_specta_builder();

    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app, _api| {
            use tauri::Manager;
            specta_builder.mount_events(app);
            app.manage(app_lock::LockRuntime::default());
            app_lock::forward_lock_events(app.app_handle());
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

    fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::App<R> {
        builder
            .plugin(init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
    }

    #[test]
    fn test_local_auth_plugin_loads() {
        let _app = create_app(tauri::test::mock_builder());
    }
}
