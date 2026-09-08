use crate::app_error::AppCommandError;
use crate::cerebro::{
    self, CerebroAuthState, CerebroPairingPoll, CerebroPairingStart, CerebroRunnerAccess,
};

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_get_auth_state() -> Result<CerebroAuthState, AppCommandError> {
    cerebro::get_auth_state().await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_start_pairing(
    cerebro_base_url: String,
) -> Result<CerebroPairingStart, AppCommandError> {
    cerebro::start_pairing(&cerebro_base_url).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_poll_pairing(handle: String) -> Result<CerebroPairingPoll, AppCommandError> {
    cerebro::poll_pairing(&handle).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_cancel_pairing(handle: String) -> Result<CerebroAuthState, AppCommandError> {
    cerebro::cancel_pairing(&handle).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_forget_runner() -> Result<CerebroAuthState, AppCommandError> {
    cerebro::forget_runner_credential()
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_refresh_access_token() -> Result<CerebroRunnerAccess, AppCommandError> {
    cerebro::refresh_access_token().await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_get_storage_settings() -> Result<cerebro::credential_storage::StorageSettings, AppCommandError> {
    cerebro::credential_storage::get_settings()
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_select_storage(mode: cerebro::credential_storage::StorageMode) -> Result<cerebro::credential_storage::StorageSettings, AppCommandError> {
    cerebro::identity::select_credential_storage(mode).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_import_credential() -> Result<(), AppCommandError> {
    cerebro::identity::import_runner_credential()
}

pub async fn cerebro_query_folder_configuration_core(db: &crate::db::AppDatabase, folder_id: i32) -> Result<cerebro::configuration::FolderConfigurationState, AppCommandError> {
    cerebro::configuration::query(&db.conn, folder_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn cerebro_query_folder_configuration(db: tauri::State<'_, crate::db::AppDatabase>, folder_id: i32) -> Result<cerebro::configuration::FolderConfigurationState, AppCommandError> {
    cerebro_query_folder_configuration_core(&db, folder_id).await
}

pub async fn cerebro_save_folder_configuration_core(db: &crate::db::AppDatabase, emitter: &crate::web::event_bridge::EventEmitter, folder_id: i32, input: cerebro::configuration::ConfigurationInput) -> Result<cerebro::configuration::ClientConfiguration, AppCommandError> {
    cerebro::configuration::save(&db.conn, emitter, folder_id, input).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn cerebro_save_folder_configuration(app: tauri::AppHandle, db: tauri::State<'_, crate::db::AppDatabase>, folder_id: i32, input: cerebro::configuration::ConfigurationInput) -> Result<cerebro::configuration::ClientConfiguration, AppCommandError> {
    cerebro_save_folder_configuration_core(&db, &crate::web::event_bridge::EventEmitter::Tauri(app), folder_id, input).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_folder_credential(folder_id: i32, rotate: bool) -> Result<cerebro::configuration::FolderCredential, AppCommandError> {
    cerebro::configuration::credential(folder_id, rotate).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_configuration_projects(page: u32, search: Option<String>) -> Result<cerebro::configuration::ProjectPage, AppCommandError> {
    cerebro::identity::RunnerIdentity::current()?.configuration_request("projects/list", &serde_json::json!({"page": page, "page_size": 100, "search": search})).await
}

#[cfg_attr(feature = "tauri-runtime", tauri::command)]
pub async fn cerebro_configuration_modules(project_id: String) -> Result<cerebro::configuration::ModuleOptions, AppCommandError> {
    cerebro::identity::RunnerIdentity::current()?.configuration_request("modules/list", &serde_json::json!({"project_id": project_id})).await
}


pub async fn cerebro_resolve_target_core(db: &crate::db::AppDatabase, target_id: String) -> Result<i32, AppCommandError> {
    let runner_id = cerebro::get_auth_state().await?.runner_id.ok_or_else(|| AppCommandError::configuration_missing("客户端尚未连接服务端"))?;
    Ok(cerebro::target_projection::resolve_folder_reference(&db.conn, &runner_id, &target_id).await?.id)
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn cerebro_resolve_target(db: tauri::State<'_, crate::db::AppDatabase>, target_id: String) -> Result<i32, AppCommandError> {
    cerebro_resolve_target_core(&db, target_id).await
}
