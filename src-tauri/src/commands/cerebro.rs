use crate::app_error::AppCommandError;
use crate::cerebro::{
    self, CerebroAuthState, CerebroPairingPoll, CerebroPairingStart, CerebroRunnerAccess,
};

pub async fn cerebro_query_launch_binding_core(
    db: &crate::db::AppDatabase,
    folder_id: i32,
    conversation_id: Option<i32>,
    work_task_id: Option<i32>,
) -> Result<cerebro::session_binding::CerebroLaunchBinding, AppCommandError> {
    cerebro::session_binding::query_launch_binding(&db.conn, folder_id, conversation_id, work_task_id).await
}

#[cfg(feature = "tauri-runtime")]
#[tauri::command]
pub async fn cerebro_query_launch_binding(
    db: tauri::State<'_, crate::db::AppDatabase>,
    folder_id: i32,
    conversation_id: Option<i32>,
    work_task_id: Option<i32>,
) -> Result<cerebro::session_binding::CerebroLaunchBinding, AppCommandError> {
    cerebro_query_launch_binding_core(&db, folder_id, conversation_id, work_task_id).await
}

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
