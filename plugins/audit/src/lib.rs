//! Settings -> Audit (ASPC D1.6): list and verify the audit log, plus the
//! few events only the webview sees (a meeting opened, an Ask query run,
//! a settings write). There is no command that edits or deletes entries.
//!
//! The desktop app opens `audit.db` and passes the reader here. It is kept
//! in a state type private to this crate, so no other plugin can read the
//! log; the append-only [`anlg_audit_log::AuditWriter`] is managed by the app.

mod commands;

use anlg_audit_log::AuditReader;
use tauri::Manager;

const PLUGIN_NAME: &str = "audit";

struct ReaderState(Option<AuditReader>);

fn make_specta_builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
    tauri_specta::Builder::<R>::new()
        .plugin_name(PLUGIN_NAME)
        .commands(tauri_specta::collect_commands![
            commands::list_entries::<tauri::Wry>,
            commands::verify::<tauri::Wry>,
            commands::record_meeting_view::<tauri::Wry>,
            commands::record_ask::<tauri::Wry>,
            commands::record_settings_change::<tauri::Wry>,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

pub fn init<R: tauri::Runtime>(reader: Option<AuditReader>) -> tauri::plugin::TauriPlugin<R> {
    let specta_builder = make_specta_builder();

    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app, _| {
            app.manage(ReaderState(reader));
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
            .plugin(init(None))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
    }
}
