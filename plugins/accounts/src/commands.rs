use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::{Account, AccountsState, Error, Provider};

#[tauri::command]
#[specta::specta]
pub(crate) fn configured_providers<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Vec<Provider> {
    app.state::<AccountsState>().configured_providers()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn list_accounts<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
) -> Result<Vec<Account>, Error> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )
    ?;
    Ok(app.state::<AccountsState>().list()?)
}

/// Opens the provider's consent page in the default browser and waits (up to
/// five minutes) for the user to approve.
#[tauri::command]
#[specta::specta]
pub(crate) async fn connect_account<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    provider: Provider,
) -> Result<ConnectedAccount, Error> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Keys,
        anlg_aspc::Action::Create,
    )?;
    let manager = app.state::<AccountsState>().inner().clone();
    let previous = manager.last_connected(provider);
    let connected_before = manager.list().unwrap_or_default();
    let opener = app.clone();
    let account = manager
        .connect(provider, move |url| {
            opener
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })
        .await?;
    let replaced =
        anlg_local_accounts::replaced_account(previous.as_ref(), &account, &connected_before);
    if let Some(replaced) = &replaced {
        tracing::warn!(
            provider = provider.key(),
            previous_account_id = %replaced.id,
            account_id = %account.id,
            "local_account_changed_on_reconnect"
        );
    }
    Ok(ConnectedAccount { account, replaced })
}

/// The account that signed in, plus the one it seems to replace when a
/// reconnect landed on a different account (see `replaced_account`).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ConnectedAccount {
    pub account: Account,
    pub replaced: Option<Account>,
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cancel_connect<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    app.state::<AccountsState>().cancel_connect();
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn disconnect_account<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::Window<R>,
    account_id: String,
) -> Result<(), Error> {
    anlg_aspc::require(
        &app,
        &window,
        anlg_aspc::DataType::Keys,
        anlg_aspc::Action::Delete,
    )?;
    let manager = app.state::<AccountsState>().inner().clone();
    manager.disconnect(&account_id).await?;
    Ok(())
}
