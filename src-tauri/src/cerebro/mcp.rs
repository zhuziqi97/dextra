//! Cerebro 的临时 MCP 启动配置；凭据由父连接 IPC 按固定作用域签发。

use super::identity;
use crate::acp::{
    delegation::listener::{CompanionCredentials, TokenEntry, TokenRegistry},
    error::AcpError,
    manager::{AdditionalMcpServers, ConnectionManager, ScopedMcpServers},
};
use crate::app_error::{AppCommandError, AppErrorCode};
use async_trait::async_trait;
use sacp::schema::{McpServer, McpServerStdio};
use std::{path::PathBuf, sync::Arc};

enum PrincipalTarget {
    Session {
        binding_id: String,
        session_id: String,
    },
    Task(String),
}

struct PrincipalScope {
    identity: identity::RunnerIdentity,
    target: PrincipalTarget,
}

#[async_trait]
impl CompanionCredentials for PrincipalScope {
    async fn issue(&self) -> Result<serde_json::Value, AppCommandError> {
        let principal = match &self.target {
            PrincipalTarget::Session {
                binding_id,
                session_id,
            } => self.identity.session_principal(binding_id, session_id).await?,
            PrincipalTarget::Task(task_id) => self.identity.task_principal(task_id).await?,
        };
        Ok(serde_json::json!(principal))
    }
}

struct CerebroLaunch {
    tokens: Arc<TokenRegistry>,
    socket_path: PathBuf,
    binary: PathBuf,
    scope: Arc<PrincipalScope>,
}

#[async_trait]
impl ScopedMcpServers for CerebroLaunch {
    async fn prepare(&self, parent_connection_id: &str) -> Result<Vec<McpServer>, AcpError> {
        let token = uuid::Uuid::new_v4().to_string();
        self.tokens
            .register_credentials(
                token.clone(),
                TokenEntry {
                    parent_connection_id: parent_connection_id.to_string(),
                    working_dir: PathBuf::new(),
                },
                self.scope.clone(),
            )
            .await;
        Ok(vec![McpServer::Stdio(
            McpServerStdio::new("cerebro", &self.binary).args(vec![
                "--socket-path".into(),
                self.socket_path.to_string_lossy().into_owned(),
                "--token".into(),
                token,
            ]),
        )])
    }
}

fn launch(
    manager: &ConnectionManager,
    scope: PrincipalScope,
) -> Result<AdditionalMcpServers, AppCommandError> {
    let (tokens, socket_path) = manager.companion_transport().ok_or_else(|| {
        AppCommandError::new(
            AppErrorCode::ConfigurationMissing,
            "伴生进程 IPC 尚未初始化",
        )
    })?;
    let binary = std::env::current_exe()
        .map_err(|error| AppCommandError::new(AppErrorCode::IoError, error.to_string()))?
        .with_file_name(if cfg!(windows) {
            "cerebro-mcp-bridge.exe"
        } else {
            "cerebro-mcp-bridge"
        });
    // tauri dev 将真实 sidecar 放在源码 binaries，安装包则使用可执行文件同目录。
    #[cfg(debug_assertions)]
    let binary = if binary.is_file() {
        binary
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(format!(
                "cerebro-mcp-bridge-{}{}",
                env!("DEXTRA_TARGET_TRIPLE"),
                if cfg!(windows) { ".exe" } else { "" }
            ))
    };
    if !binary
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    {
        return Err(AppCommandError::new(
            AppErrorCode::DependencyMissing,
            format!(
                "缺少 Cerebro MCP Bridge：{}，请构建或重新安装 Dextra",
                binary.display()
            ),
        ));
    }
    Ok(AdditionalMcpServers::scoped(Arc::new(CerebroLaunch {
        tokens,
        socket_path,
        binary,
        scope: Arc::new(scope),
    })))
}

pub async fn server_for_binding(
    manager: &ConnectionManager,
    binding_id: &str,
    session_id: &str,
) -> Result<AdditionalMcpServers, AppCommandError> {
    let identity = identity::RunnerIdentity::current()?;
    identity.session_principal(binding_id, session_id).await?;
    launch(
        manager,
        PrincipalScope { identity, target: PrincipalTarget::Session {
            binding_id: binding_id.to_string(),
            session_id: session_id.to_string(),
        } },
    )
}

pub async fn server_for_selection(
    manager: &ConnectionManager,
    selection: &super::session_binding::CerebroSelection,
    session_id: &str,
) -> Result<AdditionalMcpServers, AppCommandError> {
    let servers = match selection {
        super::session_binding::CerebroSelection::Local => Ok(Vec::new().into()),
        super::session_binding::CerebroSelection::Binding { binding_id } => {
            server_for_binding(manager, binding_id, session_id).await
        }
    }?;
    Ok(servers)
}

pub async fn server_for_task(
    manager: &ConnectionManager,
    task_id: &str,
) -> Result<AdditionalMcpServers, AppCommandError> {
    let identity = identity::RunnerIdentity::current()?;
    identity.task_principal(task_id).await?;
    launch(manager, PrincipalScope { identity, target: PrincipalTarget::Task(task_id.to_string()) })
}
