use anlg_calendar_interface::{
    CalendarEvent, CalendarListItem, CalendarProviderType, CreateEventInput, EventFilter,
};
use tauri::Manager;
use tauri_plugin_accounts::AccountsPluginExt;
use tauri_plugin_auth::AuthPluginExt;
#[cfg(target_os = "macos")]
use tauri_plugin_permissions::PermissionsPluginExt;

use crate::error::Error;

#[tauri::command]
#[specta::specta]
pub fn available_providers() -> Vec<CalendarProviderType> {
    anlg_calendar::available_providers()
}

#[tauri::command]
#[specta::specta]
pub async fn is_provider_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    provider: CalendarProviderType,
) -> Result<bool, Error> {
    if local_connection_ids(&app, provider).is_some_and(|ids| !ids.is_empty()) {
        return Ok(true);
    }
    let config = app.state::<crate::PluginConfig>();
    let token = match provider {
        CalendarProviderType::Apple => None,
        _ => access_token(&app)?,
    };
    let apple = is_apple_authorized(&app).await?;
    anlg_calendar::is_provider_enabled(&config.api_base_url, token.as_deref(), apple, provider)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn list_connection_ids<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
) -> Result<Vec<anlg_calendar::ProviderConnectionIds>, Error> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| Error::Auth(denied.to_string()))?;
    let config = app.state::<crate::PluginConfig>();
    let token = access_token(&app)?;
    let apple = is_apple_authorized(&app).await?;
    let mut providers =
        anlg_calendar::list_connection_ids(&config.api_base_url, token.as_deref(), apple).await?;
    // Accounts connected on this Mac sit alongside any server connections.
    for provider in [CalendarProviderType::Google, CalendarProviderType::Outlook] {
        let Some(local_ids) = local_connection_ids(&app, provider) else {
            continue;
        };
        match providers
            .iter_mut()
            .find(|entry| entry.provider == provider)
        {
            Some(entry) => entry.connection_ids.extend(local_ids),
            None => providers.push(anlg_calendar::ProviderConnectionIds {
                provider,
                connection_ids: local_ids,
            }),
        }
    }
    Ok(providers)
}

#[tauri::command]
#[specta::specta]
pub async fn list_calendars<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    provider: CalendarProviderType,
    connection_id: String,
) -> Result<Vec<CalendarListItem>, Error> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| Error::Auth(denied.to_string()))?;
    if let Some(account_id) = anlg_calendar::direct::local_account_id(&connection_id) {
        let calendars = with_local_token(&app, provider, account_id, |token| async move {
            anlg_calendar::direct::list_calendars(provider, &token).await
        })
        .await?;
        // Which account's mailbox answered, by opaque id and count only.
        tracing::info!(
            ?provider,
            account_id,
            calendars = calendars.len(),
            "local_account_calendars_listed"
        );
        return Ok(calendars);
    }
    let config = app.state::<crate::PluginConfig>();
    let token = match provider {
        CalendarProviderType::Apple => String::new(),
        _ => require_access_token(&app)?,
    };
    anlg_calendar::list_calendars(&config.api_base_url, &token, provider, &connection_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub async fn list_events<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    provider: CalendarProviderType,
    connection_id: String,
    filter: EventFilter,
) -> Result<Vec<CalendarEvent>, Error> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Read,
    )
    .map_err(|denied| Error::Auth(denied.to_string()))?;
    if let Some(account_id) = anlg_calendar::direct::local_account_id(&connection_id) {
        return with_local_token(&app, provider, account_id, |token| {
            let filter = filter.clone();
            async move { anlg_calendar::direct::list_events(provider, &token, filter).await }
        })
        .await;
    }
    let config = app.state::<crate::PluginConfig>();
    let token = match provider {
        CalendarProviderType::Apple => String::new(),
        _ => require_access_token(&app)?,
    };
    anlg_calendar::list_events(
        &config.api_base_url,
        &token,
        provider,
        &connection_id,
        filter,
    )
    .await
    .map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub fn open_calendar<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    provider: CalendarProviderType,
) -> Result<(), Error> {
    anlg_calendar::open_calendar(provider).map_err(Into::into)
}

#[tauri::command]
#[specta::specta]
pub fn create_event<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    provider: CalendarProviderType,
    input: CreateEventInput,
) -> Result<String, Error> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )
    .map_err(|denied| Error::Auth(denied.to_string()))?;
    anlg_calendar::create_event(provider, input).map_err(Into::into)
}

fn contacts_runtime<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<tauri_plugin_db::ManagedState, String> {
    app.try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn create_human<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::CreateHumanRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::create_human(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn create_organization<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::CreateOrganizationRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Create,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::create_organization(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn save_personal_contact<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::SavePersonalContactRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::save_personal_contact(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn update_human<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::UpdateHumanRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::update_human(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn update_organization<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::UpdateOrganizationRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::update_organization(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn soft_delete_contact<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::SoftDeleteContactRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::soft_delete_contact(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn update_contact_avatar<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::UpdateContactAvatarRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::update_contact_avatar(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn update_human_contact_summary<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::UpdateHumanContactSummaryRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::update_human_contact_summary(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn toggle_contact_pin<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::ToggleContactPinRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::toggle_contact_pin(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn reorder_pinned_contacts<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::ReorderPinnedContactsRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::reorder_pinned_contacts(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn merge_humans<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::MergeHumansRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::merge_humans(runtime.pool(), request).await
}
#[tauri::command]
#[specta::specta]
pub async fn apply_contact_enhancement<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::contacts::ApplyContactEnhancementRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = contacts_runtime(&app)?;
    let _guard = runtime.synced_write_guard().await;
    crate::contacts::apply_contact_enhancement(runtime.pool(), request).await
}

fn account_provider(provider: CalendarProviderType) -> Option<tauri_plugin_accounts::Provider> {
    match provider {
        CalendarProviderType::Google => Some(tauri_plugin_accounts::Provider::Google),
        CalendarProviderType::Outlook => Some(tauri_plugin_accounts::Provider::Microsoft),
        CalendarProviderType::Apple => None,
    }
}

/// `local:<id>` connection ids for the provider's accounts connected on this
/// Mac; `None` when the provider's sign-in isn't set up in this build.
fn local_connection_ids<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    provider: CalendarProviderType,
) -> Option<Vec<String>> {
    let account_provider = account_provider(provider)?;
    let configured = app
        .configured_account_providers()
        .contains(&account_provider);
    let accounts = app.connected_accounts().unwrap_or_else(|error| {
        tracing::warn!(%error, "local_accounts_unavailable");
        Vec::new()
    });
    let ids: Vec<String> = accounts
        .into_iter()
        .filter(|account| account.provider == account_provider)
        .map(|account| {
            format!(
                "{}{}",
                anlg_calendar::direct::LOCAL_CONNECTION_PREFIX,
                account.id
            )
        })
        .collect();
    (configured || !ids.is_empty()).then_some(ids)
}

/// Runs a provider call with the account's token, retrying once with a fresh
/// token if the provider rejects the cached one.
async fn with_local_token<R, T, F, Fut>(
    app: &tauri::AppHandle<R>,
    provider: CalendarProviderType,
    account_id: &str,
    call: F,
) -> Result<T, Error>
where
    R: tauri::Runtime,
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, anlg_calendar::Error>>,
{
    let account = app
        .connected_accounts()?
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| anlg_local_accounts::Error::UnknownAccount(account_id.to_string()))?;
    if Some(account.provider) != account_provider(provider) {
        return Err(anlg_calendar::Error::Api("account belongs to another provider".into()).into());
    }
    let token = app.account_access_token(account_id).await?;
    match call(token).await {
        Err(anlg_calendar::Error::Unauthorized) => {
            app.forget_account_access_token(account_id);
            let token = app.account_access_token(account_id).await?;
            call(token).await.map_err(Into::into)
        }
        result => result.map_err(Into::into),
    }
}

fn access_token<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<Option<String>, Error> {
    app.access_token()
        .map(|token| token.filter(|token| !token.is_empty()))
        .map_err(|error| Error::Auth(error.to_string()))
}

fn require_access_token<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<String, Error> {
    let token = access_token(app)?;
    match token {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(anlg_calendar::Error::NotAuthenticated.into()),
    }
}

async fn is_apple_authorized<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, Error> {
    #[cfg(target_os = "macos")]
    {
        let status = app
            .permissions()
            .check(tauri_plugin_permissions::Permission::Calendar)
            .await
            .map_err(|e| anlg_calendar::Error::Api(e.to_string()))?;
        Ok(matches!(
            status,
            tauri_plugin_permissions::PermissionStatus::Authorized
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(false)
    }
}

#[tauri::command]
#[specta::specta]
pub async fn apply_calendar_inventory<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::storage::ApplyCalendarInventoryRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let _guard = runtime.synced_write_guard().await;
    crate::storage::apply_calendar_inventory(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub async fn tombstone_calendar_connection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::storage::TombstoneCalendarConnectionRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Delete,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let _guard = runtime.synced_write_guard().await;
    crate::storage::tombstone_calendar_connection(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_calendar_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::storage::SetCalendarEnabledRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let _guard = runtime.synced_write_guard().await;
    crate::storage::set_calendar_enabled(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub async fn sync_calendar_connection_events<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::storage::SyncCalendarConnectionEventsRequest,
) -> Result<(), String> {
    anlg_aspc::require_ignoring_lock(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Notes,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let _guard = runtime.synced_write_guard().await;
    crate::storage::sync_calendar_connection_events(runtime.pool(), request).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_ignored_calendar_item<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    aspc_window: tauri::Window<R>,
    request: crate::storage::UpdateIgnoredCalendarItemRequest,
) -> Result<(), String> {
    anlg_aspc::require(
        &aspc_window,
        &aspc_window,
        anlg_aspc::DataType::Settings,
        anlg_aspc::Action::Edit,
    )
    .map_err(|denied| denied.to_string())?;
    let runtime = app
        .try_state::<tauri_plugin_db::ManagedState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "database is not ready yet".to_string())?;
    let _guard = runtime.synced_write_guard().await;
    crate::storage::update_ignored_calendar_item(runtime.pool(), request).await
}
