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

struct PrincipalScope {
    identity: identity::RunnerIdentity,
    target_id: String,
}

#[async_trait]
impl CompanionCredentials for PrincipalScope {
    async fn issue(&self) -> Result<serde_json::Value, AppCommandError> {
        let credential = super::configuration::credential_for_target(&self.identity, &self.target_id, false).await?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&credential.expires_at)
            .map_err(|error| AppCommandError::configuration_invalid(error.to_string()))?;
        let expires_in = (expires_at.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds().max(0) as u64;
        Ok(serde_json::json!(identity::CerebroMcpPrincipal {
            mcp_url: credential.mcp_url, access_token: credential.access_token,
            token_type: "bearer".into(), expires_in,
        }))
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

pub async fn server_for_folder(
    conn: &sea_orm::DatabaseConnection,
    manager: &ConnectionManager,
    folder_id: i32,
) -> Result<AdditionalMcpServers, AppCommandError> {
    let state = super::configuration::query(conn, folder_id).await?;
    if let Some(error) = state.error {
        tracing::warn!("[cerebro] 服务端当前不可用，继续本地会话: {error}");
        return Ok(Vec::new().into());
    }
    let Some(configuration) = state.configuration else {
        return Ok(Vec::new().into());
    };
    launch(manager, PrincipalScope { identity: identity::RunnerIdentity::current()?, target_id: configuration.target_id })
}

pub async fn server_for_conversation(
    conn: &sea_orm::DatabaseConnection,
    manager: &ConnectionManager,
    working_dir: Option<&str>,
    conversation_id: Option<i32>,
) -> Result<AdditionalMcpServers, AppCommandError> {
    use crate::db::entities::{conversation, folder};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let folder_id = if let Some(id) = conversation_id {
        conversation::Entity::find_by_id(id).one(conn).await.map_err(crate::db::error::DbError::from)?.map(|row| row.folder_id)
    } else if let Some(path) = working_dir {
        folder::Entity::find().filter(folder::Column::Path.eq(path)).filter(folder::Column::DeletedAt.is_null())
            .one(conn).await.map_err(crate::db::error::DbError::from)?.map(|row| row.id)
    } else { None };
    match folder_id {
        Some(id) => server_for_folder(conn, manager, id).await,
        None => Ok(Vec::new().into()),
    }
}
