//! HTTP handlers for the dextra-mcp service status indicator — the web-mode
//! mirror of the Tauri commands in `commands::mcp_service`.
//!
//! Both transports call the same `_core` helpers, so the probe, the state
//! ladder and the rebind behave identically whether the workspace is running
//! in the desktop shell or against a remote `dextra-server`.

use std::sync::Arc;

use axum::{extract::Extension, Json};
use serde::Deserialize;

use crate::app_error::AppCommandError;
use crate::app_state::AppState;
use crate::commands::mcp_service::{
    dextra_mcp_service_status_core, set_dextra_mcp_tool_group_core, start_dextra_mcp_service_core,
    DextraMcpServiceStatus, DextraMcpStatusSources, DextraMcpToolGroupTargets,
};

pub async fn get_dextra_mcp_service_status(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<DextraMcpServiceStatus>, AppCommandError> {
    Ok(Json(
        dextra_mcp_service_status_core(DextraMcpStatusSources {
            broker: &state.delegation_broker,
            tokens: &state.delegation_tokens,
            feedback: &state.feedback_config,
            question: &state.question_config,
            session_info: &state.session_info_config,
            browser: &state.browser_tools_config,
            authoring: &state.chat_authoring_config,
        })
        .await,
    ))
}

pub async fn start_dextra_mcp_service() -> Result<Json<()>, AppCommandError> {
    start_dextra_mcp_service_core().await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct SetDextraMcpToolGroupParams {
    pub key: String,
    pub enabled: bool,
}

pub async fn set_dextra_mcp_tool_group(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<SetDextraMcpToolGroupParams>,
) -> Result<Json<()>, AppCommandError> {
    set_dextra_mcp_tool_group_core(
        &state.db.conn,
        DextraMcpToolGroupTargets {
            broker: &state.delegation_broker,
            feedback: &state.feedback_config,
            question: &state.question_config,
            session_info: &state.session_info_config,
            browser: &state.browser_tools_config,
            authoring: &state.chat_authoring_config,
        },
        &state.emitter,
        &params.key,
        params.enabled,
    )
    .await?;
    Ok(Json(()))
}
