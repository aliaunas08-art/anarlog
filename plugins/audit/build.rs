const COMMANDS: &[&str] = &[
    "list_entries",
    "verify",
    "record_meeting_view",
    "record_ask",
    "record_settings_change",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
