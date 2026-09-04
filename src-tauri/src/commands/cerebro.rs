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
