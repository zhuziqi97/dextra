//! Cerebro owner-only 远程工作台到 Codeg 生产能力的临时 relay。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::acp::manager::ConnectionManager;
use crate::cerebro_bridge::{channel_policy, command_policy, CommandRoute};
use crate::commands::{acp, folders, terminal, work_task};
use crate::db::AppDatabase;
use crate::models::agent::AgentType;
use crate::models::WorkTaskInfo;
use crate::terminal::manager::{SpawnOptions, TerminalManager};
use crate::web::event_bridge::{EventEmitter, WebEventBroadcaster};

pub type RemoteOutbound = mpsc::Sender<Value>;

pub struct CerebroRemoteRuntime {
    pub(super) db: AppDatabaseRef,
    pub(super) connection_manager: ConnectionManager,
    pub(super) terminal_manager: TerminalManager,
    pub(super) emitter: EventEmitter,
    pub(super) event_broadcaster: Arc<WebEventBroadcaster>,
    pub(super) data_dir: PathBuf,
}

impl Clone for CerebroRemoteRuntime {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            connection_manager: self.connection_manager.clone_ref(),
            terminal_manager: self.terminal_manager.clone_ref(),
            emitter: self.emitter.clone(),
            event_broadcaster: self.event_broadcaster.clone(),
            data_dir: self.data_dir.clone(),
        }
    }
}

#[derive(Clone)]
pub(super) struct AppDatabaseRef {
    pub(super) conn: DatabaseConnection,
}

impl AppDatabaseRef {
    fn as_database(&self) -> AppDatabase {
        AppDatabase {
            conn: self.conn.clone(),
        }
    }
}

impl CerebroRemoteRuntime {
    pub fn new(
        conn: DatabaseConnection,
        connection_manager: ConnectionManager,
        terminal_manager: TerminalManager,
        emitter: EventEmitter,
        event_broadcaster: Arc<WebEventBroadcaster>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            db: AppDatabaseRef { conn },
            connection_manager,
            terminal_manager,
            emitter,
            event_broadcaster,
            data_dir,
        }
    }
}

#[derive(Clone)]
pub struct RemoteRelay {
    runtime: CerebroRemoteRuntime,
    routes: Arc<Mutex<HashMap<String, Arc<RemoteRoute>>>>,
}

struct RemoteRoute {
    folder_id: i32,
    root_path: String,
    terminal_ids: Mutex<HashSet<String>>,
    connection_ids: Mutex<HashSet<String>>,
    subscriptions: Mutex<HashMap<String, JoinHandle<()>>>,
    event_sequence: AtomicU64,
}

impl RemoteRoute {
    fn new(folder_id: i32, root_path: String) -> Self {
        Self {
            folder_id,
            root_path,
            terminal_ids: Mutex::new(HashSet::new()),
            connection_ids: Mutex::new(HashSet::new()),
            subscriptions: Mutex::new(HashMap::new()),
            event_sequence: AtomicU64::new(0),
        }
    }

    async fn stop(&self, runtime: &CerebroRemoteRuntime) {
        for (_, handle) in self.subscriptions.lock().await.drain() {
            handle.abort();
        }
        for terminal_id in self.terminal_ids.lock().await.drain() {
            let _ = runtime.terminal_manager.kill(&terminal_id);
        }
        for connection_id in self.connection_ids.lock().await.drain() {
            let _ = runtime.connection_manager.disconnect(&connection_id).await;
        }
    }

    async fn owns_terminal(&self, terminal_id: &str) -> bool {
        self.terminal_ids.lock().await.contains(terminal_id)
    }

    async fn owns_connection(&self, connection_id: &str) -> bool {
        self.connection_ids.lock().await.contains(connection_id)
    }
}

#[derive(Debug)]
struct RemoteCallError {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl RemoteCallError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_REMOTE_REQUEST",
            message: message.into(),
            retryable: false,
        }
    }

    fn core(message: impl Into<String>) -> Self {
        Self {
            code: "CORE_OPERATION_FAILED",
            message: message.into(),
            retryable: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct RemoteEnvelope {
    #[serde(rename = "TYPE")]
    message_type: String,
    #[serde(rename = "CORRELATION_ID")]
    correlation_id: String,
    #[serde(rename = "PAYLOAD")]
    payload: RemotePayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct RemotePayload {
    remote_session_id: String,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    local_work_task_ref: Option<String>,
    #[serde(default)]
    command_identity: Option<RemoteCommandIdentity>,
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct RemoteCommandIdentity {
    kind: String,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilePathArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileTreeArgs {
    #[serde(default)]
    max_depth: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameFileArgs {
    path: String,
    new_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveFileArgs {
    source_path: String,
    dest_dir: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFileArgs {
    path: String,
    name: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitStatusArgs {
    #[serde(default)]
    show_all_untracked: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitDiffArgs {
    #[serde(default)]
    file: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitLogArgs {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    skip: Option<u32>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    all_branches: Option<bool>,
    #[serde(default)]
    with_files: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitFilesArgs {
    files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitCommitArgs {
    message: String,
    files: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForgeMergeArgs {
    request: crate::forge::ChangeMergeRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSpawnArgs {
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    initial_command: Option<String>,
    #[serde(default)]
    terminal_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalIdArgs {
    terminal_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalWriteArgs {
    terminal_id: String,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalResizeArgs {
    terminal_id: String,
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskIdArgs {
    id: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskEventsArgs {
    task_id: i32,
    #[serde(default = "default_event_limit")]
    limit: u64,
}

fn default_event_limit() -> u64 {
    500
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskDiffArgs {
    id: i32,
    #[serde(default)]
    file: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskReturnArgs {
    id: i32,
    feedback: String,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    blocks: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskMergeArgs {
    id: i32,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    delete_worktree: bool,
    #[serde(default)]
    instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkTaskCompleteArgs {
    id: i32,
    #[serde(default)]
    delete_worktree: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpConnectArgs {
    agent_type: AgentType,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    preferred_mode_id: Option<String>,
    #[serde(default)]
    preferred_config_values: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionIdArgs {
    connection_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpPromptArgs {
    connection_id: String,
    blocks: Vec<crate::acp::types::PromptInputBlock>,
    #[serde(default)]
    conversation_id: Option<i32>,
    #[serde(default)]
    client_message_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpPermissionArgs {
    connection_id: String,
    request_id: String,
    option_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpQuestionArgs {
    connection_id: String,
    question_id: String,
    answer: crate::acp::question::QuestionAnswer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcpPlanApprovalArgs {
    connection_id: String,
    approval_id: String,
    answer: crate::acp::plan_approval::PlanApprovalAnswer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionArgs {
    subscription_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamAttachArgs {
    subscription_id: String,
    connection_id: String,
    #[serde(default)]
    since_seq: Option<u64>,
}

impl RemoteRelay {
    pub fn new(runtime: CerebroRemoteRuntime) -> Self {
        Self {
            runtime,
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn close_all(&self) {
        let routes = self
            .routes
            .lock()
            .await
            .drain()
            .map(|(_, route)| route)
            .collect::<Vec<_>>();
        for route in routes {
            route.stop(&self.runtime).await;
        }
    }

    pub async fn handle_server_message(
        &self,
        runner_id: &str,
        raw: Value,
        outbound: RemoteOutbound,
    ) -> bool {
        let message_type = raw.get("TYPE").and_then(Value::as_str);
        if !matches!(
            message_type,
            Some("REMOTE_ATTACH" | "REMOTE_REQUEST" | "REMOTE_DETACH")
        ) {
            return false;
        }
        let envelope = match serde_json::from_value::<RemoteEnvelope>(raw) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!("[cerebro] invalid remote command: {error}");
                return true;
            }
        };
        match envelope.message_type.as_str() {
            "REMOTE_ATTACH" => {
                self.attach(runner_id, envelope, outbound).await;
            }
            "REMOTE_REQUEST" => {
                let relay = self.clone();
                let runner_id = runner_id.to_string();
                tokio::spawn(async move {
                    relay.request(&runner_id, envelope, outbound).await;
                });
            }
            "REMOTE_DETACH" => {
                self.detach(runner_id, envelope, outbound).await;
            }
            _ => unreachable!("remote type checked above"),
        }
        true
    }

    async fn attach(&self, runner_id: &str, envelope: RemoteEnvelope, outbound: RemoteOutbound) {
        let session_id = envelope.payload.remote_session_id.clone();
        let result = async {
            let target_id = envelope
                .payload
                .target_id
                .as_deref()
                .ok_or_else(|| RemoteCallError::invalid("TARGET_ID is required"))?;
            let folder_id = super::target_projection::resolve_background_task_folder(
                &self.runtime.db.conn,
                runner_id,
                target_id,
            )
            .await
            .map_err(|error| RemoteCallError::core(error.to_string()))?;
            let db = self.runtime.db.as_database();
            let folder = folders::get_folder_core(&db, folder_id)
                .await
                .map_err(|error| RemoteCallError::core(error.to_string()))?;
            let projection = super::project_folder_targets(&self.runtime.db.conn, runner_id)
                .await
                .map_err(|error| RemoteCallError::core(error.to_string()))?
                .into_iter()
                .find(|target| target.target_id == target_id)
                .ok_or_else(|| RemoteCallError::core("target disappeared while attaching"))?;
            let route = Arc::new(RemoteRoute::new(folder_id, folder.path));
            if let Some(previous) = self.routes.lock().await.insert(session_id.clone(), route) {
                previous.stop(&self.runtime).await;
            }
            let runner_hello =
                crate::cerebro_bridge::runner_hello(runner_id, env!("CARGO_PKG_VERSION"));
            Ok(json!({
                "TYPE": "REMOTE_ATTACHED",
                "CORRELATION_ID": envelope.correlation_id,
                "PAYLOAD": {
                    "REMOTE_SESSION_ID": session_id,
                    "SNAPSHOT": {
                        "type": "workspace",
                        "rootPath": "/workspace",
                        "workspace": projection.workspace_display_name,
                        "repository": projection.repository,
                        "branch": projection.branch,
                        "workTaskId": envelope.payload.local_work_task_ref,
                        "runnerHello": runner_hello,
                        "status": "READY"
                    }
                }
            }))
        }
        .await;

        let value = match result {
            Ok(value) => value,
            Err(error) => remote_error_envelope(
                "REMOTE_ATTACH_FAILED",
                runner_id,
                &envelope.correlation_id,
                &session_id,
                error,
                None,
            ),
        };
        let _ = outbound.send(with_runner_fields(value, runner_id)).await;
    }

    async fn request(&self, runner_id: &str, envelope: RemoteEnvelope, outbound: RemoteOutbound) {
        let session_id = envelope.payload.remote_session_id.clone();
        let result = async {
            let route = self
                .routes
                .lock()
                .await
                .get(&session_id)
                .cloned()
                .ok_or_else(|| RemoteCallError::invalid("remote session is not attached"))?;
            let identity = envelope
                .payload
                .command_identity
                .as_ref()
                .ok_or_else(|| RemoteCallError::invalid("COMMAND_IDENTITY is required"))?;
            match identity.kind.as_str() {
                "CALL" => {
                    self.dispatch_call(&route, &identity.name, envelope.payload.arguments)
                        .await
                }
                "CHANNEL_SUBSCRIBE" => {
                    self.subscribe_channel(
                        &session_id,
                        route,
                        &identity.name,
                        envelope.payload.arguments,
                        runner_id,
                        outbound.clone(),
                    )
                    .await
                }
                "CHANNEL_UNSUBSCRIBE" => self.unsubscribe(route, envelope.payload.arguments).await,
                "STREAM_ATTACH" => {
                    self.attach_stream(
                        &session_id,
                        route,
                        envelope.payload.arguments,
                        runner_id,
                        outbound.clone(),
                    )
                    .await
                }
                "STREAM_DETACH" => self.unsubscribe(route, envelope.payload.arguments).await,
                _ => Err(RemoteCallError::invalid("unknown remote command kind")),
            }
        }
        .await;

        let value = match result {
            Ok(result) => json!({
                "TYPE": "REMOTE_RESPONSE",
                "CORRELATION_ID": envelope.correlation_id,
                "PAYLOAD": {"REMOTE_SESSION_ID": session_id, "RESULT": result}
            }),
            Err(error) => remote_error_envelope(
                "REMOTE_RESPONSE",
                runner_id,
                &envelope.correlation_id,
                &session_id,
                error,
                None,
            ),
        };
        let _ = outbound.send(with_runner_fields(value, runner_id)).await;
    }

    async fn detach(&self, runner_id: &str, envelope: RemoteEnvelope, outbound: RemoteOutbound) {
        let session_id = envelope.payload.remote_session_id;
        if let Some(route) = self.routes.lock().await.remove(&session_id) {
            route.stop(&self.runtime).await;
        }
        let value = json!({
            "TYPE": "REMOTE_DETACHED",
            "CORRELATION_ID": envelope.correlation_id,
            "PAYLOAD": {"REMOTE_SESSION_ID": session_id, "REASON": "CLIENT_DETACHED"}
        });
        let _ = outbound.send(with_runner_fields(value, runner_id)).await;
    }

    async fn dispatch_call(
        &self,
        route: &Arc<RemoteRoute>,
        command: &str,
        args: Value,
    ) -> Result<Value, RemoteCallError> {
        let policy = command_policy(command).ok_or_else(|| RemoteCallError {
            code: "CODEG_COMMAND_NOT_REMOTE",
            message: format!("command {command} is not registered for remote use"),
            retryable: false,
        })?;
        match policy.route {
            CommandRoute::Deny => {
                return Err(RemoteCallError {
                    code: "CODEG_COMMAND_REMOTE_DENIED",
                    message: format!("command group {} is not available remotely", policy.group),
                    retryable: false,
                });
            }
            CommandRoute::Operation => {
                return Err(RemoteCallError {
                    code: "CODEG_COMMAND_REQUIRES_OPERATION",
                    message: format!("command {command} requires a platform operation"),
                    retryable: false,
                });
            }
            CommandRoute::Relay => {}
        }

        let db = self.runtime.db.as_database();
        let root = route.root_path.clone();
        match command {
            "get_file_tree" => {
                let p: FileTreeArgs = decode(args)?;
                encode(folders::get_file_tree(root, p.max_depth).await)
            }
            "read_file_preview" => {
                let p: FilePathArgs = decode(args)?;
                encode(folders::read_file_preview(root, p.path).await)
            }
            "read_file_for_edit" => {
                let p: FilePathArgs = decode(args)?;
                encode(folders::read_file_for_edit(root, p.path).await)
            }
            "save_file_content" => {
                let p: SaveFileArgs = decode(args)?;
                encode(folders::save_file_content(root, p.path, p.content, None).await)
            }
            "save_file_copy" => {
                let p: SaveFileArgs = decode(args)?;
                encode(folders::save_file_copy(root, p.path, p.content).await)
            }
            "rename_file_tree_entry" => {
                let p: RenameFileArgs = decode(args)?;
                encode(folders::rename_file_tree_entry(root, p.path, p.new_name).await)
            }
            "move_file_tree_entry" => {
                let p: MoveFileArgs = decode(args)?;
                encode(folders::move_file_tree_entry(root, p.source_path, p.dest_dir).await)
            }
            "delete_file_tree_entry" => {
                let p: FilePathArgs = decode(args)?;
                encode(folders::delete_file_tree_entry(root, p.path).await)
            }
            "create_file_tree_entry" => {
                let p: CreateFileArgs = decode(args)?;
                encode(folders::create_file_tree_entry(root, p.path, p.name, p.kind).await)
            }
            "git_status" => {
                let p: GitStatusArgs = decode(args)?;
                encode(folders::git_status(root, p.show_all_untracked).await)
            }
            "git_diff" => {
                let p: GitDiffArgs = decode(args)?;
                encode(folders::git_diff(root, p.file).await)
            }
            "git_log" => {
                let p: GitLogArgs = decode(args)?;
                encode(
                    folders::git_log(
                        root,
                        p.limit,
                        p.branch,
                        p.remote,
                        p.skip,
                        p.author,
                        p.all_branches,
                        p.with_files,
                    )
                    .await,
                )
            }
            "git_add_files" => {
                let p: GitFilesArgs = decode(args)?;
                encode(folders::git_add_files(root, p.files).await)
            }
            "git_commit" => {
                let p: GitCommitArgs = decode(args)?;
                encode(
                    folders::git_commit_core(
                        &self.runtime.emitter,
                        Some(route.folder_id),
                        &self.runtime.db.conn,
                        &root,
                        &p.message,
                        &p.files,
                    )
                    .await,
                )
            }
            "forge_merge_change" => {
                let p: ForgeMergeArgs = decode(args)?;
                encode(
                    crate::commands::forge::forge_merge_change_core(
                        &db,
                        route.folder_id,
                        p.request,
                    )
                    .await,
                )
            }
            "terminal_spawn" => {
                let p: TerminalSpawnArgs = decode(args)?;
                let terminal_id = p
                    .terminal_id
                    .filter(|id| !id.is_empty() && id.len() <= 256)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let result = self
                    .runtime
                    .terminal_manager
                    .spawn_with_id(
                        SpawnOptions {
                            terminal_id: terminal_id.clone(),
                            working_dir: root,
                            owner_window_label: format!("cerebro:remote:{}", route.folder_id),
                            shell: p.shell,
                            initial_command: p.initial_command,
                            extra_env: terminal::prepare_credential_env(&self.runtime.data_dir),
                            temp_files: vec![],
                        },
                        self.runtime.emitter.clone(),
                    )
                    .map_err(|error| RemoteCallError::core(error.to_string()))?;
                route.terminal_ids.lock().await.insert(result.clone());
                encode(Ok::<_, String>(result))
            }
            "terminal_write" => {
                let p: TerminalWriteArgs = decode(args)?;
                require_terminal(route, &p.terminal_id).await?;
                encode(
                    self.runtime
                        .terminal_manager
                        .write(&p.terminal_id, p.data.as_bytes()),
                )
            }
            "terminal_resize" => {
                let p: TerminalResizeArgs = decode(args)?;
                require_terminal(route, &p.terminal_id).await?;
                encode(
                    self.runtime
                        .terminal_manager
                        .resize(&p.terminal_id, p.cols, p.rows),
                )
            }
            "terminal_kill" => {
                let p: TerminalIdArgs = decode(args)?;
                require_terminal(route, &p.terminal_id).await?;
                let value = encode(self.runtime.terminal_manager.kill(&p.terminal_id))?;
                route.terminal_ids.lock().await.remove(&p.terminal_id);
                Ok(value)
            }
            "terminal_list" => {
                let owned = route.terminal_ids.lock().await.clone();
                let terminals = self
                    .runtime
                    .terminal_manager
                    .list_with_exit_check(Some(&self.runtime.emitter))
                    .into_iter()
                    .filter(|terminal| owned.contains(&terminal.id))
                    .collect::<Vec<_>>();
                encode(Ok::<_, String>(terminals))
            }
            "work_task_list" => {
                encode(work_task::work_task_list_core(&db, Some(route.folder_id)).await)
            }
            "work_task_get" => {
                let p: WorkTaskIdArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(remote_work_task_get(&db, p.id, &route.root_path).await)
            }
            "work_task_events" => {
                let p: WorkTaskEventsArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.task_id).await?;
                encode(work_task::work_task_events_core(&db, p.task_id, p.limit).await)
            }
            "work_task_diff" => {
                let p: WorkTaskDiffArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(work_task::work_task_diff_core(&db, p.id, p.file).await)
            }
            "work_task_changed_files" => {
                let p: WorkTaskIdArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(work_task::work_task_changed_files_core(&db, p.id).await)
            }
            "work_task_return" => {
                let p: WorkTaskReturnArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(work_task::work_task_return_core(p.id, p.feedback, p.intent, p.blocks).await)
            }
            "work_task_merge" => {
                let p: WorkTaskMergeArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(
                    work_task::work_task_merge_core(
                        p.id,
                        p.message,
                        p.delete_worktree,
                        p.instructions,
                    )
                    .await,
                )
            }
            "work_task_complete" => {
                let p: WorkTaskCompleteArgs = decode(args)?;
                require_work_task(&db, route.folder_id, p.id).await?;
                encode(work_task::work_task_complete_core(p.id, p.delete_worktree).await)
            }
            "acp_connect" => {
                let p: AcpConnectArgs = decode(args)?;
                let runtime_env = acp::build_session_runtime_env(
                    &db,
                    p.agent_type,
                    p.session_id.as_deref(),
                    &self.runtime.data_dir,
                )
                .await
                .map_err(|error| RemoteCallError::core(error.to_string()))?;
                acp::verify_agent_installed(p.agent_type)
                    .await
                    .map_err(|error| RemoteCallError::core(error.to_string()))?;
                let connection_id = self
                    .runtime
                    .connection_manager
                    .spawn_agent(
                        p.agent_type,
                        Some(root),
                        p.session_id,
                        runtime_env,
                        format!("cerebro:remote:{}", route.folder_id),
                        self.runtime.emitter.clone(),
                        p.preferred_mode_id,
                        p.preferred_config_values,
                    )
                    .await
                    .map_err(|error| RemoteCallError::core(error.to_string()))?;
                route
                    .connection_ids
                    .lock()
                    .await
                    .insert(connection_id.clone());
                encode(Ok::<_, String>(connection_id))
            }
            "acp_prompt" => {
                let p: AcpPromptArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    self.runtime
                        .connection_manager
                        .send_prompt_linked_with_message_id(
                            &db,
                            &p.connection_id,
                            p.blocks,
                            Some(route.folder_id),
                            p.conversation_id,
                            None,
                            p.client_message_id,
                        )
                        .await,
                )
            }
            "acp_cancel" => {
                let p: ConnectionIdArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    self.runtime
                        .connection_manager
                        .cancel(&self.runtime.db.conn, &p.connection_id)
                        .await,
                )
            }
            "acp_disconnect" => {
                let p: ConnectionIdArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                let result = encode(
                    self.runtime
                        .connection_manager
                        .disconnect(&p.connection_id)
                        .await,
                )?;
                route.connection_ids.lock().await.remove(&p.connection_id);
                Ok(result)
            }
            "acp_respond_permission" => {
                let p: AcpPermissionArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    self.runtime
                        .connection_manager
                        .respond_permission(&p.connection_id, &p.request_id, &p.option_id)
                        .await,
                )
            }
            "acp_answer_question" => {
                let p: AcpQuestionArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    self.runtime
                        .connection_manager
                        .answer_question(&p.connection_id, &p.question_id, p.answer)
                        .await,
                )
            }
            "acp_answer_plan_approval" => {
                let p: AcpPlanApprovalArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    self.runtime
                        .connection_manager
                        .answer_plan_approval(&p.connection_id, &p.approval_id, p.answer)
                        .await,
                )
            }
            "acp_list_connections" => {
                let owned = route.connection_ids.lock().await.clone();
                let connections = self
                    .runtime
                    .connection_manager
                    .list_connections()
                    .await
                    .into_iter()
                    .filter(|connection| owned.contains(&connection.id))
                    .collect::<Vec<_>>();
                encode(Ok::<_, String>(connections))
            }
            "acp_get_session_snapshot" => {
                let p: ConnectionIdArgs = decode(args)?;
                require_connection(route, &p.connection_id).await?;
                encode(
                    acp::acp_get_session_snapshot_core(
                        &self.runtime.connection_manager,
                        &p.connection_id,
                    )
                    .await,
                )
            }
            _ => Err(RemoteCallError::core(format!(
                "registered relay command {command} has no production dispatcher"
            ))),
        }
        .map_err(|error| sanitize_error(error, &route.root_path))
    }

    async fn subscribe_channel(
        &self,
        session_id: &str,
        route: Arc<RemoteRoute>,
        channel: &str,
        args: Value,
        runner_id: &str,
        outbound: RemoteOutbound,
    ) -> Result<Value, RemoteCallError> {
        if channel_policy(channel).is_none() {
            return Err(RemoteCallError {
                code: "CODEG_CHANNEL_NOT_REMOTE",
                message: format!("channel {channel} is not registered for remote use"),
                retryable: false,
            });
        }
        if let Some(terminal_id) = channel
            .strip_prefix("terminal://output/")
            .or_else(|| channel.strip_prefix("terminal://exit/"))
        {
            require_terminal(&route, terminal_id).await?;
        }
        let p: SubscriptionArgs = decode(args)?;
        let mut receiver = self.runtime.event_broadcaster.subscribe();
        let requested_channel = channel.to_string();
        let subscription_id = p.subscription_id.clone();
        let session_id = session_id.to_string();
        let runner_id = runner_id.to_string();
        let route_for_task = route.clone();
        let handle = tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                if event.channel != requested_channel {
                    continue;
                }
                let frame = json!({
                    "type": "channel_event",
                    "subscriptionId": subscription_id,
                    "channel": event.channel,
                    "payload": event.payload,
                });
                if send_remote_event(&outbound, &runner_id, &session_id, &route_for_task, frame)
                    .await
                    .is_err()
                {
                    return;
                }
            }
        });
        if let Some(previous) = route
            .subscriptions
            .lock()
            .await
            .insert(p.subscription_id, handle)
        {
            previous.abort();
        }
        Ok(json!({"subscribed": true}))
    }

    async fn attach_stream(
        &self,
        session_id: &str,
        route: Arc<RemoteRoute>,
        args: Value,
        runner_id: &str,
        outbound: RemoteOutbound,
    ) -> Result<Value, RemoteCallError> {
        let p: StreamAttachArgs = decode(args)?;
        require_connection(&route, &p.connection_id).await?;
        let event_bus = self
            .runtime
            .emitter
            .acp_event_bus()
            .ok_or_else(|| RemoteCallError::core("ACP event bus is unavailable"))?;
        let metrics = event_bus.metrics();
        let outcome = crate::web::ws_attach::handle_attach(
            &self.runtime.connection_manager,
            metrics,
            p.subscription_id.clone(),
            p.connection_id,
            p.since_seq,
        )
        .await
        .map_err(|reason| RemoteCallError::core(format!("stream attach failed: {reason:?}")))?;
        let initial = serde_json::to_value(outcome.initial_msg)
            .map_err(|error| RemoteCallError::core(error.to_string()))?;
        send_remote_event(&outbound, runner_id, session_id, &route, initial).await?;

        let subscription_id = p.subscription_id.clone();
        let session_id = session_id.to_string();
        let runner_id = runner_id.to_string();
        let route_for_task = route.clone();
        let mut receiver = outcome.receiver;
        let handle = tokio::spawn(async move {
            loop {
                let frame = match receiver.recv().await {
                    Ok(envelope) => json!({
                        "type": "event",
                        "subscription_id": subscription_id,
                        "envelope": envelope,
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => json!({
                        "type": "detached",
                        "subscription_id": subscription_id,
                        "reason": "lagged",
                    }),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => json!({
                        "type": "detached",
                        "subscription_id": subscription_id,
                        "reason": "connection_gone",
                    }),
                };
                let terminal = frame.get("type").and_then(Value::as_str) == Some("detached");
                if send_remote_event(&outbound, &runner_id, &session_id, &route_for_task, frame)
                    .await
                    .is_err()
                    || terminal
                {
                    return;
                }
            }
        });
        if let Some(previous) = route
            .subscriptions
            .lock()
            .await
            .insert(p.subscription_id, handle)
        {
            previous.abort();
        }
        Ok(json!({"attached": true}))
    }

    async fn unsubscribe(
        &self,
        route: Arc<RemoteRoute>,
        args: Value,
    ) -> Result<Value, RemoteCallError> {
        let p: SubscriptionArgs = decode(args)?;
        if let Some(handle) = route.subscriptions.lock().await.remove(&p.subscription_id) {
            handle.abort();
        }
        Ok(json!({"detached": true}))
    }
}

async fn require_terminal(route: &RemoteRoute, terminal_id: &str) -> Result<(), RemoteCallError> {
    if route.owns_terminal(terminal_id).await {
        Ok(())
    } else {
        Err(RemoteCallError::invalid(
            "terminal does not belong to this remote session",
        ))
    }
}

async fn require_connection(
    route: &RemoteRoute,
    connection_id: &str,
) -> Result<(), RemoteCallError> {
    if route.owns_connection(connection_id).await {
        Ok(())
    } else {
        Err(RemoteCallError::invalid(
            "ACP connection does not belong to this remote session",
        ))
    }
}

async fn require_work_task(
    db: &AppDatabase,
    folder_id: i32,
    task_id: i32,
) -> Result<(), RemoteCallError> {
    let task = work_task::work_task_get_core(db, task_id)
        .await
        .map_err(|error| RemoteCallError::core(error.to_string()))?;
    if task.folder_id != folder_id {
        return Err(RemoteCallError::invalid(
            "work task does not belong to this remote workspace",
        ));
    }
    Ok(())
}

async fn remote_work_task_get(
    db: &AppDatabase,
    task_id: i32,
    root_path: &str,
) -> Result<WorkTaskInfo, String> {
    let mut task = work_task::work_task_get_core(db, task_id)
        .await
        .map_err(|error| error.to_string())?;
    let mut local_roots = vec![root_path.to_string()];
    if let Some(worktree_folder_id) = task.worktree_folder_id {
        let worktree = folders::get_folder_core(db, worktree_folder_id)
            .await
            .map_err(|error| error.to_string())?;
        local_roots.push(worktree.path);
    }
    if let Some(summary) = task.result_summary.as_deref() {
        task.result_summary = Some(super::task_link::project_result_summary(
            summary,
            &local_roots,
        ));
    }
    Ok(task)
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, RemoteCallError> {
    serde_json::from_value(value)
        .map_err(|error| RemoteCallError::invalid(format!("invalid command arguments: {error}")))
}

fn encode<T: serde::Serialize, E: std::fmt::Display>(
    result: Result<T, E>,
) -> Result<Value, RemoteCallError> {
    let value = result.map_err(|error| RemoteCallError::core(error.to_string()))?;
    serde_json::to_value(value).map_err(|error| RemoteCallError::core(error.to_string()))
}

fn sanitize_error(mut error: RemoteCallError, root_path: &str) -> RemoteCallError {
    if !root_path.is_empty() {
        error.message = error.message.replace(root_path, "/workspace");
    }
    error
}

fn with_runner_fields(mut value: Value, runner_id: &str) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("PROTOCOL_VERSION".into(), json!(1));
        object.insert("MESSAGE_ID".into(), json!(uuid::Uuid::new_v4().to_string()));
        object.insert("OCCURRED_AT".into(), json!(chrono::Utc::now().to_rfc3339()));
        object.insert("RUNNER_ID".into(), json!(runner_id));
    }
    value
}

fn remote_error_envelope(
    message_type: &str,
    runner_id: &str,
    correlation_id: &str,
    session_id: &str,
    error: RemoteCallError,
    sequence: Option<u64>,
) -> Value {
    let mut value = json!({
        "TYPE": message_type,
        "CORRELATION_ID": correlation_id,
        "PAYLOAD": {
            "REMOTE_SESSION_ID": session_id,
            "ERROR": {
                "CODE": error.code,
                "MESSAGE": error.message,
                "RETRYABLE": error.retryable,
                "DETAILS": {}
            }
        }
    });
    if let Some(sequence) = sequence {
        value["SEQUENCE"] = json!(sequence);
    }
    with_runner_fields(value, runner_id)
}

async fn send_remote_event(
    outbound: &RemoteOutbound,
    runner_id: &str,
    session_id: &str,
    route: &RemoteRoute,
    frame: Value,
) -> Result<(), RemoteCallError> {
    let sequence = route.event_sequence.fetch_add(1, Ordering::AcqRel) + 1;
    outbound
        .send(with_runner_fields(
            json!({
                "TYPE": "REMOTE_EVENT",
                "SEQUENCE": sequence,
                "PAYLOAD": {"REMOTE_SESSION_ID": session_id, "FRAME": frame}
            }),
            runner_id,
        ))
        .await
        .map_err(|_| RemoteCallError::core("Runner connection closed"))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::acp::{EventBusMetrics, InternalEventBus};
    use crate::db::service::folder_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};

    fn init_repo(path: &std::path::Path) {
        std::fs::create_dir_all(path).unwrap();
        let output = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    async fn fixture() -> (
        RemoteRelay,
        RemoteOutbound,
        mpsc::Receiver<Value>,
        TempDir,
        String,
    ) {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("workspace");
        init_repo(&repo);
        std::fs::write(repo.join("owned.txt"), "owner workspace").unwrap();

        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, repo.to_str().unwrap()).await;
        folder_service::update_folder_default_agent(&db.conn, folder_id, Some(AgentType::Codex))
            .await
            .unwrap();
        let broadcaster = Arc::new(WebEventBroadcaster::new());
        let bus = Arc::new(InternalEventBus::new(Arc::new(EventBusMetrics::default())));
        let emitter = EventEmitter::web_only(broadcaster.clone(), bus);
        let runtime = CerebroRemoteRuntime::new(
            db.conn,
            ConnectionManager::new(),
            TerminalManager::new(),
            emitter,
            broadcaster,
            temp.path().join("data"),
        );
        let (outbound, receiver) = mpsc::channel(16);
        (
            RemoteRelay::new(runtime),
            outbound,
            receiver,
            temp,
            super::super::target_projection::target_id("runner-1", folder_id),
        )
    }

    fn attach_frame(target_id: &str) -> Value {
        json!({
            "TYPE": "REMOTE_ATTACH",
            "CORRELATION_ID": "attach-1",
            "PAYLOAD": {
                "REMOTE_SESSION_ID": "session-1",
                "TARGET_ID": target_id
            }
        })
    }

    fn request_frame(request_id: &str, command: &str, arguments: Value) -> Value {
        json!({
            "TYPE": "REMOTE_REQUEST",
            "CORRELATION_ID": request_id,
            "PAYLOAD": {
                "REMOTE_SESSION_ID": "session-1",
                "COMMAND_IDENTITY": {"KIND": "CALL", "NAME": command},
                "ARGUMENTS": arguments
            }
        })
    }

    #[tokio::test]
    async fn attach_hides_local_path_and_calls_stay_inside_bound_target() {
        let (relay, outbound, mut receiver, temp, target_id) = fixture().await;
        assert!(
            relay
                .handle_server_message("runner-1", attach_frame(&target_id), outbound.clone())
                .await
        );
        let attached = receiver.recv().await.unwrap();
        assert_eq!(attached["TYPE"], "REMOTE_ATTACHED");
        assert_eq!(attached["PAYLOAD"]["SNAPSHOT"]["rootPath"], "/workspace");
        assert!(!attached.to_string().contains(temp.path().to_str().unwrap()));

        relay
            .handle_server_message(
                "runner-1",
                request_frame(
                    "read-1",
                    "read_file_for_edit",
                    json!({
                        "rootPath": temp.path().join("other"),
                        "path": "owned.txt"
                    }),
                ),
                outbound.clone(),
            )
            .await;
        let response = receiver.recv().await.unwrap();
        assert_eq!(response["TYPE"], "REMOTE_RESPONSE");
        assert_eq!(response["PAYLOAD"]["RESULT"]["content"], "owner workspace");

        relay
            .handle_server_message(
                "runner-1",
                request_frame(
                    "save-1",
                    "save_file_content",
                    json!({
                        "rootPath": temp.path().join("other"),
                        "path": "owned.txt",
                        "content": "changed from remote workbench"
                    }),
                ),
                outbound.clone(),
            )
            .await;
        let saved = receiver.recv().await.unwrap();
        assert_eq!(saved["TYPE"], "REMOTE_RESPONSE");
        assert!(saved["PAYLOAD"].get("ERROR").is_none());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("workspace/owned.txt")).unwrap(),
            "changed from remote workbench"
        );

        relay
            .handle_server_message(
                "runner-1",
                request_frame("denied-1", "acp_update_agent_env", json!({})),
                outbound.clone(),
            )
            .await;
        let denied = receiver.recv().await.unwrap();
        assert_eq!(
            denied["PAYLOAD"]["ERROR"]["CODE"],
            "CODEG_COMMAND_REMOTE_DENIED"
        );

        relay
            .handle_server_message(
                "runner-1",
                request_frame("read-2", "read_file_for_edit", json!({"path": "owned.txt"})),
                outbound,
            )
            .await;
        let after_error = receiver.recv().await.unwrap();
        assert_eq!(
            after_error["PAYLOAD"]["RESULT"]["content"],
            "changed from remote workbench"
        );
    }
}
