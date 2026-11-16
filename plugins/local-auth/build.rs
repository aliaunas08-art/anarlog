const COMMANDS: &[&str] = &[
    "available",
    "authenticate",
    "access_context",
    "app_lock_state",
    "unlock_app",
    "lock_app",
    "set_app_lock_enabled",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
