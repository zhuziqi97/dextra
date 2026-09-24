//! 服务端目录配置的客户端 adapter；本地只保存展示缓存。

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use crate::app_error::AppCommandError;
use crate::db::service::app_metadata_service;
use crate::web::event_bridge::{emit_event, EventEmitter};
use super::{identity, target_projection};

pub const CONFIGURATION_EVENT: &str = "cerebro://configuration-changed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub enum MCPPermission { READ, WRITE }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct MCPScopeItem {
    pub module_id: String,
    pub permission: MCPPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct MCPCapabilities {
    // 公共 schema 规定省略新能力开关时为 false。
    #[serde(default)]
    pub code_graph_enabled: bool,
    #[serde(default)]
    pub forge_enabled: bool,
    #[serde(default)]
    pub issue_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct MCPScopeDetail {
    pub module_id: String,
    pub module_name: String,
    pub project_id: String,
    pub project_name: String,
    pub can_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ClientConfiguration {
    pub runner_id: String,
    pub target_id: String,
    pub execution_module_id: Option<String>,
    pub execution_project: Option<ProjectOption>,
    pub binding_id: Option<String>,
    pub mcp_scope_modules: Vec<MCPScopeItem>,
    pub mcp_capabilities: MCPCapabilities,
    pub mcp_enabled: bool,
    pub mcp_scope_details: Vec<MCPScopeDetail>,
    pub module_labels: std::collections::BTreeMap<String, String>,
    pub grant_id: Option<String>,
    pub credential_expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ConfigurationInput {
    pub execution_module_id: Option<String>,
    pub mcp_scope_modules: Vec<MCPScopeItem>,
    pub mcp_capabilities: MCPCapabilities,
    pub mcp_enabled: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct FolderConfigurationState {
    pub configuration: Option<ClientConfiguration>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FolderCredential {
    pub mcp_url: String,
    pub access_token: String,
    pub expires_at: String,
}

fn cache_key(target_id: &str) -> String { format!("cerebro.folder_configuration:{target_id}") }

pub(super) async fn cache(conn: &DatabaseConnection, configuration: &ClientConfiguration) -> Result<(), AppCommandError> {
    let value = serde_json::to_string(configuration).map_err(|error| AppCommandError::configuration_invalid(error.to_string()))?;
    app_metadata_service::upsert_value(conn, &cache_key(&configuration.target_id), &value).await.map_err(AppCommandError::db)
}

pub async fn cached(conn: &DatabaseConnection, target_id: &str) -> Result<Option<ClientConfiguration>, AppCommandError> {
    app_metadata_service::get_value(conn, &cache_key(target_id)).await.map_err(AppCommandError::db)?
        .map(|value| serde_json::from_str(&value).map_err(|error| AppCommandError::configuration_invalid(error.to_string()))).transpose()
}

pub async fn refresh(conn: &DatabaseConnection, target_id: &str) -> Result<ClientConfiguration, AppCommandError> {
    let configuration = identity::RunnerIdentity::current()?.configuration_request("query", &serde_json::json!({"target_id": target_id})).await?;
    cache(conn, &configuration).await?;
    Ok(configuration)
}

pub async fn query(conn: &DatabaseConnection, folder_id: i32) -> Result<FolderConfigurationState, AppCommandError> {
    let auth = identity::get_auth_state().await?;
    let Some(runner_id) = auth.runner_id else { return Ok(FolderConfigurationState { configuration: None, error: None }); };
    let target_id = target_projection::target_id(&runner_id, folder_id);
    if cached(conn, &target_id).await?.is_none() {
        if let Err(error) = super::connection::synchronize_targets().await {
            return Ok(FolderConfigurationState { configuration: None, error: Some(error) });
        }
    }
    match refresh(conn, &target_id).await {
        Ok(configuration) => Ok(FolderConfigurationState { configuration: Some(configuration), error: None }),
        Err(error) => Ok(FolderConfigurationState { configuration: cached(conn, &target_id).await?, error: Some(error.to_string()) }),
    }
}

pub async fn save(conn: &DatabaseConnection, emitter: &EventEmitter, folder_id: i32, input: ConfigurationInput) -> Result<ClientConfiguration, AppCommandError> {
    let auth = identity::get_auth_state().await?;
    let runner_id = auth.runner_id.ok_or_else(|| AppCommandError::configuration_missing("请先连接服务端"))?;
    let target_id = target_projection::target_id(&runner_id, folder_id);
    let configuration: ClientConfiguration = identity::RunnerIdentity::current()?.configuration_request("save", &serde_json::json!({
        "target_id": target_id, "execution_module_id": input.execution_module_id,
        "mcp_scope_modules": input.mcp_scope_modules, "mcp_capabilities": input.mcp_capabilities, "mcp_enabled": input.mcp_enabled,
    })).await?;
    // 服务端已提交；缓存写入故障不能把成功保存变成失败。
    if let Err(error) = cache(conn, &configuration).await { tracing::warn!("[cerebro] 保存配置缓存失败: {error}"); }
    emit_event(emitter, CONFIGURATION_EVENT, &configuration);
    Ok(configuration)
}

pub async fn credential_for_target(identity: &identity::RunnerIdentity, target_id: &str, rotate: bool) -> Result<FolderCredential, AppCommandError> {
    identity.configuration_request("credential", &serde_json::json!({"target_id": target_id, "rotate": rotate})).await
}

pub async fn refresh_and_emit(conn: &DatabaseConnection, emitter: &EventEmitter, target_id: &str) {
    match refresh(conn, target_id).await {
        Ok(configuration) => emit_event(emitter, CONFIGURATION_EVENT, configuration),
        Err(error) => tracing::warn!("[cerebro] 刷新目录配置失败: {error}"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ProjectOption {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ProjectPage {
    pub items: Vec<ProjectOption>,
    pub total: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ModuleOption {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub path: String,
    pub can_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ModuleOptions {
    pub items: Vec<ModuleOption>,
    pub truncated: bool,
}
