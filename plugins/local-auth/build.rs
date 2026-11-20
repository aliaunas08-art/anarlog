const COMMANDS: &[&str] = &[
    "available",
    "authenticate",
    "access_context",
    "app_lock_state",
    "unlock_app",
    "lock_app",
    "set_app_lock_enabled",
    "app_lock_settings",
    "set_app_lock_idle_minutes",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
