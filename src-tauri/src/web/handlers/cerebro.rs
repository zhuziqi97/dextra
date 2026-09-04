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
