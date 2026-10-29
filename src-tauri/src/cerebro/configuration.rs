//! 服务端目录配置的客户端 adapter；本地只保存展示缓存。

use super::{identity, target_projection};
use crate::app_error::AppCommandError;
use crate::db::service::app_metadata_service;
use crate::web::event_bridge::{emit_event, EventEmitter};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

pub const CONFIGURATION_EVENT: &str = "cerebro://configuration-changed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub enum MCPPermission {
    READ,
    WRITE,
}

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
pub struct ProjectReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ClientProject {
    pub client_project_id: String,
    pub runner_id: String,
    pub native_project_kind: String,
    pub native_project_id: String,
    pub logical_project_id: Option<String>,
    pub display_name: String,
    pub execution_host_id: String,
    pub availability: String,
    pub unavailable_reason: Option<ProjectReason>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ExecutionTarget {
    pub target_id: String,
    pub runner_id: String,
    pub client_project_id: String,
    pub native_workspace_key: String,
    pub execution_host_id: String,
    pub path: Option<String>,
    pub display_name: String,
    pub workspace_display_name: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub runner_status: String,
    pub availability: String,
    pub unavailable_reason: Option<ProjectReason>,
    pub agent_type: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ModuleBinding {
    pub binding_id: String,
    pub module_id: String,
    pub runner_id: String,
    pub client_project_id: String,
    pub target_id: Option<String>,
    pub status: String,
    pub target: Option<ExecutionTarget>,
    pub created_at: String,
    pub updated_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct ClientConfiguration {
    pub runner_id: String,
    pub runner_name: String,
    pub client_type: String,
    pub client_project_id: String,
    pub project: ClientProject,
    pub targets: Vec<ExecutionTarget>,
    pub bindings: Vec<ModuleBinding>,
    pub mcp_scope_modules: Vec<MCPScopeItem>,
    pub mcp_capabilities: MCPCapabilities,
    pub mcp_enabled: bool,
    pub mcp_scope_details: Vec<MCPScopeDetail>,
    pub binding_module_details: Vec<MCPScopeDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ModuleConfiguration {
    pub bindings: Vec<ModuleBinding>,
    pub active_binding_id: Option<String>,
    pub configurations: Vec<ClientConfiguration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct BindingOperation {
    pub configuration: ClientConfiguration,
    pub module: ModuleConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ConfigurationInput {
    pub mcp_scope_modules: Vec<MCPScopeItem>,
    pub mcp_capabilities: MCPCapabilities,
    pub mcp_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum BindingInput {
    Bind {
        module_id: String,
    },
    Unbind {
        binding_id: String,
    },
    SelectTarget {
        binding_id: String,
        target_id: Option<String>,
    },
    Activate {
        binding_id: String,
        target_id: String,
    },
    Deactivate {
        binding_id: String,
    },
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

fn cache_key(runner_id: &str, native_project_id: &str) -> String {
    format!("cerebro.project_configuration:{runner_id}:dextra_folder:{native_project_id}")
}

pub(super) async fn cache(
    conn: &DatabaseConnection,
    configuration: &ClientConfiguration,
) -> Result<(), AppCommandError> {
    if configuration.client_type != "dextra"
        || configuration.project.native_project_kind != "dextra_folder"
        || configuration.project.runner_id != configuration.runner_id
    {
        return Err(AppCommandError::configuration_invalid(
            "配置不属于 Dextra 项目",
        ));
    }
    let value = serde_json::to_string(configuration)
        .map_err(|error| AppCommandError::configuration_invalid(error.to_string()))?;
    app_metadata_service::upsert_value(
        conn,
        &cache_key(
            &configuration.runner_id,
            &configuration.project.native_project_id,
        ),
        &value,
    )
    .await
    .map_err(AppCommandError::db)
}

pub async fn cached(
    conn: &DatabaseConnection,
    runner_id: &str,
    root_id: i32,
) -> Result<Option<ClientConfiguration>, AppCommandError> {
    app_metadata_service::get_value(conn, &cache_key(runner_id, &root_id.to_string()))
        .await
        .map_err(AppCommandError::db)?
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| AppCommandError::configuration_invalid(error.to_string()))
        })
        .transpose()
}

pub async fn refresh(
    conn: &DatabaseConnection,
    project_id: &str,
) -> Result<ClientConfiguration, AppCommandError> {
    let configuration = identity::RunnerIdentity::current()?
        .configuration_request(
            "query",
            &serde_json::json!({"client_project_id": project_id}),
        )
        .await?;
    cache(conn, &configuration).await?;
    Ok(configuration)
}

pub async fn query(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<FolderConfigurationState, AppCommandError> {
    let auth = identity::get_auth_state().await?;
    let Some(runner_id) = auth.runner_id else {
        return Ok(FolderConfigurationState {
            configuration: None,
            error: None,
        });
    };
    let root_id = target_projection::project_root_for_folder(conn, folder_id)
        .await?
        .id;
    if cached(conn, &runner_id, root_id).await?.is_none() {
        if let Err(error) = super::connection::synchronize_targets().await {
            return Ok(FolderConfigurationState {
                configuration: None,
                error: Some(error),
            });
        }
    }
    let previous = cached(conn, &runner_id, root_id).await?;
    let Some(project) = &previous else {
        return Ok(FolderConfigurationState {
            configuration: None,
            error: Some("项目报告尚未确认".into()),
        });
    };
    match refresh(conn, &project.client_project_id).await {
        Ok(configuration) => Ok(FolderConfigurationState {
            configuration: Some(configuration),
            error: None,
        }),
        Err(error) => Ok(FolderConfigurationState {
            configuration: previous,
            error: Some(error.to_string()),
        }),
    }
}

async fn require_project(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<ClientConfiguration, AppCommandError> {
    let state = query(conn, folder_id).await?;
    if let Some(error) = state.error {
        return Err(AppCommandError::configuration_invalid(error));
    }
    state
        .configuration
        .ok_or_else(|| AppCommandError::configuration_missing("请先连接并等待项目报告确认"))
}

async fn committed(
    conn: &DatabaseConnection,
    emitter: &EventEmitter,
    configuration: &ClientConfiguration,
) {
    if let Err(error) = cache(conn, configuration).await {
        tracing::warn!("[cerebro] 保存项目配置缓存失败: {error}");
    }
    emit_event(emitter, CONFIGURATION_EVENT, configuration);
}

pub async fn save(
    conn: &DatabaseConnection,
    emitter: &EventEmitter,
    folder_id: i32,
    input: ConfigurationInput,
) -> Result<ClientConfiguration, AppCommandError> {
    let project = require_project(conn, folder_id).await?;
    let configuration = identity::RunnerIdentity::current()?.configuration_request("save", &serde_json::json!({
        "client_project_id": project.client_project_id,
        "mcp_scope_modules": input.mcp_scope_modules, "mcp_capabilities": input.mcp_capabilities, "mcp_enabled": input.mcp_enabled,
    })).await?;
    committed(conn, emitter, &configuration).await;
    Ok(configuration)
}

pub async fn binding(
    conn: &DatabaseConnection,
    emitter: &EventEmitter,
    folder_id: i32,
    input: BindingInput,
) -> Result<BindingOperation, AppCommandError> {
    let project = require_project(conn, folder_id).await?;
    let (operation, body) = match input {
        BindingInput::Bind { module_id } => (
            "bind",
            serde_json::json!({"client_project_id": project.client_project_id, "module_id": module_id}),
        ),
        BindingInput::Unbind { binding_id } => {
            ("unbind", serde_json::json!({"binding_id": binding_id}))
        }
        BindingInput::SelectTarget {
            binding_id,
            target_id,
        } => (
            "select-target",
            serde_json::json!({"binding_id": binding_id, "target_id": target_id}),
        ),
        BindingInput::Activate {
            binding_id,
            target_id,
        } => (
            "activate",
            serde_json::json!({"binding_id": binding_id, "target_id": target_id}),
        ),
        BindingInput::Deactivate { binding_id } => {
            ("deactivate", serde_json::json!({"binding_id": binding_id}))
        }
    };
    if let Some(id) = body.get("binding_id").and_then(serde_json::Value::as_str) {
        if !project.bindings.iter().any(|item| item.binding_id == id) {
            return Err(AppCommandError::configuration_invalid("关联不属于所选项目"));
        }
    }
    let result: BindingOperation = identity::RunnerIdentity::current()?
        .configuration_request(operation, &body)
        .await?;
    for configuration in &result.module.configurations {
        if configuration.runner_id == project.runner_id {
            committed(conn, emitter, configuration).await;
        }
    }
    committed(conn, emitter, &result.configuration).await;
    Ok(result)
}

pub async fn credential_for_target(
    identity: &identity::RunnerIdentity,
    project_id: &str,
    target_id: &str,
) -> Result<FolderCredential, AppCommandError> {
    identity
        .configuration_request(
            "credential",
            &serde_json::json!({"client_project_id": project_id, "target_id": target_id}),
        )
        .await
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
