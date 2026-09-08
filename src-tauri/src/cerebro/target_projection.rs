use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::commands::folders::resolve_git_head;
use crate::db::entities::folder::{self, FolderKind};
use crate::db::error::DbError;
use crate::db::service::work_task_service;
use crate::models::WorkTaskFolderSettings;

/// Dextra 上报给 Cerebro 的 Folder Target 投影。
///
/// `target_id` 是 Runner 与稳定 Folder row 身份组成的不透明引用；本机路径和
/// 独立 Folder ID 不进入协议字段，也不需要另一张 Target identity 表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct FolderTargetProjection {
    pub target_id: String,
    pub display_name: String,
    pub workspace_display_name: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub supports_background_task: bool,
    pub background_task_unavailable_reason: Option<TargetProjectionReason>,
    pub availability: TargetAvailability,
    pub unavailable_reason: Option<TargetProjectionReason>,
    pub agent_type: Option<String>,
    pub mode_display_name: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct TargetProjectionReason {
    pub code: TargetProjectionReasonCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetProjectionReasonCode {
    FolderClosed,
    WorkspacePathMissing,
    WorkspacePathUnavailable,
    NotGitRepository,
    NotRepositoryRoot,
    GitUnavailable,
    WorkTaskSettingsUnavailable,
}

struct GitFacts {
    repository: Option<String>,
    branch: Option<String>,
    background_reason: Option<TargetProjectionReason>,
}

/// 投影所有仍存在的普通根 Folder；关闭或暂时不可用的 Folder 仍保留在完整集合中。
pub async fn project_folder_targets(
    conn: &DatabaseConnection,
    runner_id: &str,
) -> Result<Vec<FolderTargetProjection>, DbError> {
    let folders = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .filter(folder::Column::ParentId.is_null())
        .order_by_asc(folder::Column::Id)
        .all(conn)
        .await?;
    let observed_at = Utc::now();
    let mut targets = Vec::with_capacity(folders.len());

    for folder in folders {
        let settings = work_task_service::settings_get_effective(conn, folder.id).await?;
        targets.push(project_folder(runner_id, folder, settings, observed_at).await);
    }
    Ok(targets)
}

async fn project_folder(
    runner_id: &str,
    folder: folder::Model,
    settings: WorkTaskFolderSettings,
    observed_at: DateTime<Utc>,
) -> FolderTargetProjection {
    let path = Path::new(&folder.path);
    let availability_reason = availability_reason(&folder, path);
    let availability = if availability_reason.is_some() {
        TargetAvailability::Unavailable
    } else {
        TargetAvailability::Available
    };
    let git = inspect_git(path).await;
    let agent_type = settings
        .default_agent_type
        .clone()
        .or_else(|| folder.default_agent_type.clone());
    let settings_reason = agent_type.is_none().then(|| {
        reason(
            TargetProjectionReasonCode::WorkTaskSettingsUnavailable,
            "No agent is configured for background WorkTasks",
        )
    });
    let background_reason = git.background_reason.or(settings_reason);
    let mode_display_name = settings
        .label_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.get("mode_label"))
        .and_then(|label| label.as_str())
        .map(str::to_owned)
        .or_else(|| settings.mode_id.clone());

    FolderTargetProjection {
        target_id: target_id(runner_id, folder.id),
        display_name: folder.alias.clone().unwrap_or_else(|| folder.name.clone()),
        workspace_display_name: folder.name,
        repository: git.repository,
        branch: git.branch,
        supports_background_task: background_reason.is_none(),
        background_task_unavailable_reason: background_reason,
        availability,
        unavailable_reason: availability_reason,
        agent_type,
        mode_display_name,
        updated_at: observed_at,
    }
}

pub(crate) fn target_id(runner_id: &str, folder_id: i32) -> String {
    format!("dextra-target:{runner_id}:folder:{folder_id}")
}

/// 会话使用用户选中的目录，不要求 Git 仓库或仓库根目录。
pub(crate) async fn resolve_conversation_folder(
    conn: &DatabaseConnection,
    runner_id: &str,
    requested_target_id: &str,
) -> Result<i32, DbError> {
    Ok(resolve_available_folder(conn, runner_id, requested_target_id).await?.id)
}

/// 仅原生 WorkTask 启动需要后台任务能力。
pub(crate) async fn resolve_background_task_folder(
    conn: &DatabaseConnection,
    runner_id: &str,
    requested_target_id: &str,
) -> Result<i32, DbError> {
    let folder = resolve_available_folder(conn, runner_id, requested_target_id).await?;
    let id = folder.id;
    let settings = work_task_service::settings_get_effective(conn, id).await?;
    let projection = project_folder(runner_id, folder, settings, Utc::now()).await;
    if !projection.supports_background_task {
        return Err(DbError::Validation(
            projection.background_task_unavailable_reason.map_or_else(
                || "target does not support background tasks".into(),
                |reason| reason.message,
            ),
        ));
    }
    Ok(id)
}

async fn resolve_available_folder(
    conn: &DatabaseConnection,
    runner_id: &str,
    requested_target_id: &str,
) -> Result<folder::Model, DbError> {
    let folders = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .filter(folder::Column::ParentId.is_null())
        .order_by_asc(folder::Column::Id)
        .all(conn)
        .await?;
    let folder = folders
        .into_iter()
        .find(|folder| target_id(runner_id, folder.id) == requested_target_id)
        .ok_or_else(|| DbError::NotFound(format!("target {requested_target_id}")))?;
    if let Some(reason) = availability_reason(&folder, Path::new(&folder.path)) {
        return Err(DbError::Validation(reason.message));
    }
    Ok(folder)
}

fn availability_reason(folder: &folder::Model, path: &Path) -> Option<TargetProjectionReason> {
    workspace_path_reason(path).or_else(|| {
        (!folder.is_open).then(|| {
            reason(
                TargetProjectionReasonCode::FolderClosed,
                "Folder is closed in Codeg",
            )
        })
    })
}

fn workspace_path_reason(path: &Path) -> Option<TargetProjectionReason> {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(reason(
            TargetProjectionReasonCode::WorkspacePathMissing,
            "Workspace path does not exist",
        )),
        Err(_) => Some(reason(
            TargetProjectionReasonCode::WorkspacePathUnavailable,
            "Workspace path is not accessible",
        )),
        Ok(metadata) if !metadata.is_dir() => Some(reason(
            TargetProjectionReasonCode::WorkspacePathUnavailable,
            "Workspace path is not a directory",
        )),
        Ok(_) => None,
    }
}

async fn inspect_git(path: &Path) -> GitFacts {
    if let Some(path_reason) = workspace_path_reason(path) {
        return GitFacts {
            repository: None,
            branch: None,
            background_reason: Some(path_reason),
        };
    }

    let output = match crate::process::tokio_command("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .await
    {
        Ok(output) => output,
        Err(_) => {
            return GitFacts {
                repository: None,
                branch: None,
                background_reason: Some(reason(
                    TargetProjectionReasonCode::GitUnavailable,
                    "Git is not available for this workspace",
                )),
            };
        }
    };
    if !output.status.success() {
        return GitFacts {
            repository: None,
            branch: None,
            background_reason: Some(reason(
                TargetProjectionReasonCode::NotGitRepository,
                "Workspace is not inside a Git repository",
            )),
        };
    }

    let raw_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw_root.is_empty() {
        return GitFacts {
            repository: None,
            branch: None,
            background_reason: Some(reason(
                TargetProjectionReasonCode::GitUnavailable,
                "Git did not report a repository root",
            )),
        };
    }
    let repository_root = PathBuf::from(raw_root);
    let repository = repository_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let branch = resolve_git_head(&folder_path(path))
        .await
        .ok()
        .filter(|head| head.is_repo)
        .and_then(|head| head.branch);
    let is_root = paths_equal(path, &repository_root);

    GitFacts {
        repository,
        branch,
        background_reason: (!is_root).then(|| {
            reason(
                TargetProjectionReasonCode::NotRepositoryRoot,
                "Workspace is inside a Git repository but is not its root",
            )
        }),
    }
}

fn folder_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn reason(code: TargetProjectionReasonCode, message: &str) -> TargetProjectionReason {
    TargetProjectionReason {
        code,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::db::service::folder_service;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};
    use crate::models::agent::AgentType;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("运行测试 Git 命令");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "-b", "main"]);
    }

    fn target<'a>(targets: &'a [FolderTargetProjection], name: &str) -> &'a FolderTargetProjection {
        targets
            .iter()
            .find(|target| target.workspace_display_name == name)
            .unwrap_or_else(|| panic!("缺少 {name} Target"))
    }

    fn reason_code(reason: &Option<TargetProjectionReason>) -> Option<TargetProjectionReasonCode> {
        reason.as_ref().map(|reason| reason.code)
    }

    async fn configure_agent(db: &crate::db::AppDatabase, folder_id: i32) {
        folder_service::update_folder_default_agent(&db.conn, folder_id, Some(AgentType::Codex))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn projects_every_live_regular_root_and_separates_availability_from_capability() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("root-repo");
        let nested = repo.join("nested-workspace");
        let closed_repo = temp.path().join("closed-repo");
        let plain = temp.path().join("plain-folder");
        let deleted = temp.path().join("deleted-folder");
        let child = temp.path().join("engine-worktree");
        let chat = temp.path().join("chat-folder");
        let missing = temp.path().join("missing-folder");
        init_repo(&repo);
        init_repo(&closed_repo);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::create_dir_all(&deleted).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&chat).unwrap();

        let db = fresh_in_memory_db().await;
        let repo_id = seed_folder(&db, repo.to_str().unwrap()).await;
        let nested_id = seed_folder(&db, nested.to_str().unwrap()).await;
        let closed_id = seed_folder(&db, closed_repo.to_str().unwrap()).await;
        let plain_id = seed_folder(&db, plain.to_str().unwrap()).await;
        let missing_id = seed_folder(&db, missing.to_str().unwrap()).await;
        let deleted_id = seed_folder(&db, deleted.to_str().unwrap()).await;
        folder_service::add_folder_with_parent(&db.conn, child.to_str().unwrap(), Some(repo_id))
            .await
            .unwrap()
            .id;
        folder_service::add_chat_folder(&db.conn, chat.to_str().unwrap())
            .await
            .unwrap();
        for folder_id in [repo_id, nested_id, closed_id, plain_id, missing_id] {
            configure_agent(&db, folder_id).await;
        }
        folder_service::set_folder_open(&db.conn, closed_id, false)
            .await
            .unwrap();
        folder_service::soft_delete_folder(&db.conn, deleted_id)
            .await
            .unwrap();

        let targets = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        assert_eq!(targets.len(), 5);
        assert_eq!(
            targets
                .iter()
                .map(|target| target.target_id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            targets.len()
        );
        assert!(targets.iter().all(|target| {
            target.workspace_display_name != "engine-worktree"
                && target.workspace_display_name != "chat-folder"
                && target.workspace_display_name != "deleted-folder"
        }));

        let root = target(&targets, "root-repo");
        assert_eq!(root.availability, TargetAvailability::Available);
        assert_eq!(root.unavailable_reason, None);
        assert!(root.supports_background_task);
        assert_eq!(root.background_task_unavailable_reason, None);
        assert_eq!(root.repository.as_deref(), Some("root-repo"));
        assert_eq!(root.branch.as_deref(), Some("main"));

        let nested = target(&targets, "nested-workspace");
        assert_eq!(nested.availability, TargetAvailability::Available);
        assert!(!nested.supports_background_task);
        assert_eq!(
            reason_code(&nested.background_task_unavailable_reason),
            Some(TargetProjectionReasonCode::NotRepositoryRoot)
        );

        let plain = target(&targets, "plain-folder");
        assert_eq!(plain.availability, TargetAvailability::Available);
        assert_eq!(plain.unavailable_reason, None);
        assert!(!plain.supports_background_task);
        assert_eq!(plain.repository, None);
        assert_eq!(plain.branch, None);
        assert_eq!(
            reason_code(&plain.background_task_unavailable_reason),
            Some(TargetProjectionReasonCode::NotGitRepository)
        );

        let closed = target(&targets, "closed-repo");
        assert_eq!(closed.availability, TargetAvailability::Unavailable);
        assert_eq!(
            reason_code(&closed.unavailable_reason),
            Some(TargetProjectionReasonCode::FolderClosed)
        );
        assert!(closed.supports_background_task);
        assert_eq!(closed.background_task_unavailable_reason, None);

        let missing = target(&targets, "missing-folder");
        assert_eq!(missing.availability, TargetAvailability::Unavailable);
        assert_eq!(
            reason_code(&missing.unavailable_reason),
            Some(TargetProjectionReasonCode::WorkspacePathMissing)
        );
        assert!(!missing.supports_background_task);
        assert_eq!(
            reason_code(&missing.background_task_unavailable_reason),
            Some(TargetProjectionReasonCode::WorkspacePathMissing)
        );
    }

    #[tokio::test]
    async fn projects_effective_agent_mode_and_stable_opaque_identity_without_local_path() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("identity-repo");
        init_repo(&repo);
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, repo.to_str().unwrap()).await;
        configure_agent(&db, folder_id).await;
        folder_service::update_folder_alias(&db.conn, folder_id, Some("My target".into()))
            .await
            .unwrap();

        let first = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        let repeated = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        let another_runner = project_folder_targets(&db.conn, "runner-b").await.unwrap();
        assert_eq!(first[0].target_id, repeated[0].target_id);
        assert_ne!(first[0].target_id, another_runner[0].target_id);
        assert_eq!(first[0].display_name, "My target");
        assert_eq!(first[0].workspace_display_name, "identity-repo");
        assert_eq!(first[0].agent_type.as_deref(), Some("codex"));
        assert_eq!(first[0].mode_display_name, None);

        let settings = WorkTaskFolderSettings {
            default_agent_type: Some("claude_code".into()),
            mode_id: Some("plan".into()),
            ..Default::default()
        };
        work_task_service::settings_set(&db.conn, folder_id, &settings)
            .await
            .unwrap();
        let updated = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        assert_eq!(updated[0].agent_type.as_deref(), Some("claude_code"));
        assert_eq!(updated[0].mode_display_name.as_deref(), Some("plan"));

        let labeled_settings = WorkTaskFolderSettings {
            label_snapshot: Some(serde_json::json!({"mode_label": "Plan mode"})),
            ..settings
        };
        work_task_service::settings_set(&db.conn, folder_id, &labeled_settings)
            .await
            .unwrap();
        let labeled = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        assert_eq!(labeled[0].mode_display_name.as_deref(), Some("Plan mode"));

        let wire = serde_json::to_value(&labeled[0]).unwrap();
        assert!(wire.get("TARGET_ID").is_some());
        assert!(wire.get("FOLDER_ID").is_none());
        assert!(wire.get("PATH").is_none());
        assert!(!wire.to_string().contains(repo.to_str().unwrap()));
    }

    #[tokio::test]
    async fn reports_missing_agent_as_background_only_unavailability() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("no-agent-repo");
        init_repo(&repo);
        let db = fresh_in_memory_db().await;
        seed_folder(&db, repo.to_str().unwrap()).await;

        let targets = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        assert_eq!(targets[0].availability, TargetAvailability::Available);
        assert_eq!(targets[0].agent_type, None);
        assert!(!targets[0].supports_background_task);
        assert_eq!(
            reason_code(&targets[0].background_task_unavailable_reason),
            Some(TargetProjectionReasonCode::WorkTaskSettingsUnavailable)
        );
    }

    #[tokio::test]
    async fn root_filter_is_owned_by_the_production_query_not_a_test_inventory() {
        let temp = TempDir::new().unwrap();
        let db = fresh_in_memory_db().await;
        let first_path = temp.path().join("first");
        let second_path = temp.path().join("second");
        std::fs::create_dir_all(&first_path).unwrap();
        std::fs::create_dir_all(&second_path).unwrap();
        seed_folder(&db, first_path.to_str().unwrap()).await;
        let before = project_folder_targets(&db.conn, "runner-a").await.unwrap();

        seed_folder(&db, second_path.to_str().unwrap()).await;

        let after = project_folder_targets(&db.conn, "runner-a").await.unwrap();
        assert_eq!(after.len(), before.len() + 1);
        assert!(after
            .iter()
            .any(|target| target.workspace_display_name == "second"));
    }
}
