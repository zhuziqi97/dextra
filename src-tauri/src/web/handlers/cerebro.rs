use axum::Json;
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::cerebro::{
    CerebroAuthState, CerebroPairingPoll, CerebroPairingStart, CerebroRunnerAccess,
};
use crate::commands::cerebro as commands;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPairingParams {
    pub cerebro_base_url: String,
}

#[derive(Deserialize)]
pub struct PairingHandleParams {
    pub handle: String,
}

pub async fn get_auth_state() -> Result<Json<CerebroAuthState>, AppCommandError> {
    commands::cerebro_get_auth_state().await.map(Json)
}

pub async fn start_pairing(
    Json(params): Json<StartPairingParams>,
) -> Result<Json<CerebroPairingStart>, AppCommandError> {
    commands::cerebro_start_pairing(params.cerebro_base_url)
        .await
        .map(Json)
}

pub async fn poll_pairing(
    Json(params): Json<PairingHandleParams>,
) -> Result<Json<CerebroPairingPoll>, AppCommandError> {
    commands::cerebro_poll_pairing(params.handle)
        .await
        .map(Json)
}

pub async fn cancel_pairing(
    Json(params): Json<PairingHandleParams>,
) -> Result<Json<CerebroAuthState>, AppCommandError> {
    commands::cerebro_cancel_pairing(params.handle)
        .await
        .map(Json)
}

pub async fn forget_runner() -> Result<Json<CerebroAuthState>, AppCommandError> {
    commands::cerebro_forget_runner().await.map(Json)
}

pub async fn refresh_access_token() -> Result<Json<CerebroRunnerAccess>, AppCommandError> {
    commands::cerebro_refresh_access_token().await.map(Json)
}

#[derive(Deserialize)]
pub struct StorageParams {
    pub mode: crate::cerebro::credential_storage::StorageMode,
}

pub async fn get_storage_settings() -> Result<Json<crate::cerebro::credential_storage::StorageSettings>, AppCommandError> {
    commands::cerebro_get_storage_settings().await.map(Json)
}

pub async fn select_storage(Json(params): Json<StorageParams>) -> Result<Json<crate::cerebro::credential_storage::StorageSettings>, AppCommandError> {
    commands::cerebro_select_storage(params.mode).await.map(Json)
}

pub async fn import_credential() -> Result<Json<()>, AppCommandError> {
    commands::cerebro_import_credential().await.map(Json)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderConfigurationParams { pub folder_id: i32 }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderConfigurationSaveParams { pub folder_id: i32, pub input: crate::cerebro::configuration::ConfigurationInput }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderCredentialParams { pub folder_id: i32, pub rotate: bool }

#[derive(Deserialize)]
pub struct ProjectsParams { pub page: u32, pub search: Option<String> }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulesParams { pub project_id: String }

pub async fn query_folder_configuration(axum::extract::Extension(state): axum::extract::Extension<std::sync::Arc<crate::app_state::AppState>>, Json(params): Json<FolderConfigurationParams>) -> Result<Json<crate::cerebro::configuration::FolderConfigurationState>, AppCommandError> {
    commands::cerebro_query_folder_configuration_core(&state.db, params.folder_id).await.map(Json)
}

pub async fn save_folder_configuration(axum::extract::Extension(state): axum::extract::Extension<std::sync::Arc<crate::app_state::AppState>>, Json(params): Json<FolderConfigurationSaveParams>) -> Result<Json<crate::cerebro::configuration::ClientConfiguration>, AppCommandError> {
    commands::cerebro_save_folder_configuration_core(&state.db, &state.emitter, params.folder_id, params.input).await.map(Json)
}

pub async fn folder_credential(Json(params): Json<FolderCredentialParams>) -> Result<Json<crate::cerebro::configuration::FolderCredential>, AppCommandError> {
    commands::cerebro_folder_credential(params.folder_id, params.rotate).await.map(Json)
}

pub async fn configuration_projects(Json(params): Json<ProjectsParams>) -> Result<Json<crate::cerebro::configuration::ProjectPage>, AppCommandError> {
    commands::cerebro_configuration_projects(params.page, params.search).await.map(Json)
}

pub async fn configuration_modules(Json(params): Json<ModulesParams>) -> Result<Json<crate::cerebro::configuration::ModuleOptions>, AppCommandError> {
    commands::cerebro_configuration_modules(params.project_id).await.map(Json)
}


#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetParams { pub target_id: String }

pub async fn resolve_target(axum::extract::Extension(state): axum::extract::Extension<std::sync::Arc<crate::app_state::AppState>>, Json(params): Json<TargetParams>) -> Result<Json<i32>, AppCommandError> {
    commands::cerebro_resolve_target_core(&state.db, params.target_id).await.map(Json)
}
