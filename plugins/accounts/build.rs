const COMMANDS: &[&str] = &[
    "configured_providers",
    "list_accounts",
    "connect_account",
    "cancel_connect",
    "disconnect_account",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
