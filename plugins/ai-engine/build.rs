const COMMANDS: &[&str] = &[
    "request",
    "cancel_request",
    "status",
    "key_status",
    "set_api_key",
    "clear_api_key",
    "set_quotas",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
