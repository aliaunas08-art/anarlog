//! Google and Microsoft accounts the user connects from the Mac. The sign-in
//! itself lives in `anlg-local-accounts`; this plugin owns the manager, opens
//! the browser and exposes the commands. Other plugins get access tokens
//! through [`AccountsPluginExt`].

use std::sync::Arc;

use anlg_local_accounts::{AccountManager, KeychainStore};
use tauri::Manager;

pub use anlg_local_accounts::{Account, Provider};

mod commands;

const PLUGIN_NAME: &str = "accounts";

pub type AccountsState = Arc<AccountManager<KeychainStore>>;

/// Error sent to the webview: a stable `code` for the UI, plus the message.
#[derive(Debug, Clone, serde::Serialize, specta::Type, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: String,
    pub message: String,
}

impl From<anlg_local_accounts::Error> for Error {
    fn from(error: anlg_local_accounts::Error) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
        }
    }
}

pub trait AccountsPluginExt<R: tauri::Runtime> {
    /// A current access token for a connected account.
    fn account_access_token(
        &self,
        account_id: &str,
    ) -> impl std::future::Future<Output = Result<String, anlg_local_accounts::Error>> + Send;
    fn connected_accounts(&self) -> Result<Vec<Account>, anlg_local_accounts::Error>;
    fn configured_account_providers(&self) -> Vec<Provider>;
    fn forget_account_access_token(&self, account_id: &str);
}

impl<R: tauri::Runtime, T: Manager<R> + Sync> AccountsPluginExt<R> for T {
    async fn account_access_token(
        &self,
        account_id: &str,
    ) -> Result<String, anlg_local_accounts::Error> {
        let manager = self.state::<AccountsState>().inner().clone();
        manager.access_token(account_id).await
    }

    fn connected_accounts(&self) -> Result<Vec<Account>, anlg_local_accounts::Error> {
        self.state::<AccountsState>().list()
    }

    fn configured_account_providers(&self) -> Vec<Provider> {
        self.state::<AccountsState>().configured_providers()
    }

    fn forget_account_access_token(&self, account_id: &str) {
        self.state::<AccountsState>()
            .forget_access_token(account_id);
    }
}

fn make_specta_builder<R: tauri::Runtime>() -> tauri_specta::Builder<R> {
    tauri_specta::Builder::<R>::new()
        .plugin_name(PLUGIN_NAME)
        .commands(tauri_specta::collect_commands![
            commands::configured_providers::<tauri::Wry>,
            commands::list_accounts::<tauri::Wry>,
            commands::connect_account::<tauri::Wry>,
            commands::cancel_connect::<tauri::Wry>,
            commands::disconnect_account::<tauri::Wry>,
        ])
        .error_handling(tauri_specta::ErrorHandlingMode::Result)
}

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let specta_builder = make_specta_builder();

    tauri::plugin::Builder::new(PLUGIN_NAME)
        .invoke_handler(specta_builder.invoke_handler())
        .setup(|app, _api| {
            // Dev and release builds have different identifiers, so their
            // accounts stay apart in the Keychain.
            let service = format!("{}.accounts", app.config().identifier);
            app.manage::<AccountsState>(Arc::new(AccountManager::new(KeychainStore::new(service))));
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
}
