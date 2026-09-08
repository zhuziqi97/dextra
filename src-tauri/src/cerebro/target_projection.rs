//! 客户端目录事实投影；执行模块和 MCP 范围由服务端配置拥有。

use std::path::Path;
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use crate::commands::folders::resolve_git_head;
use crate::db::entities::folder::{self, FolderKind};
use crate::db::error::DbError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct FolderTargetProjection {
    pub target_id: String,
    pub display_name: String,
    pub workspace_display_name: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub availability: TargetAvailability,
    pub unavailable_reason: Option<TargetProjectionReason>,
    pub agent_type: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetAvailability { Available, Unavailable }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct TargetProjectionReason { pub code: TargetProjectionReasonCode, pub message: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TargetProjectionReasonCode { FolderClosed, WorkspacePathMissing, WorkspacePathUnavailable }

pub async fn project_folder_targets(conn: &DatabaseConnection, runner_id: &str) -> Result<Vec<FolderTargetProjection>, DbError> {
    let folders = folder::Entity::find().filter(folder::Column::DeletedAt.is_null()).filter(folder::Column::Kind.eq(FolderKind::Regular)).filter(folder::Column::ParentId.is_null()).order_by_asc(folder::Column::Id).all(conn).await?;
    let observed_at = Utc::now();
    let mut targets = Vec::with_capacity(folders.len());
    for folder in folders {
        let path = Path::new(&folder.path);
        let unavailable_reason = availability_reason(&folder, path);
        let (repository, branch) = git_display(path).await;
        targets.push(FolderTargetProjection { target_id: target_id(runner_id, folder.id), display_name: folder.alias.clone().unwrap_or_else(|| folder.name.clone()), workspace_display_name: folder.name,
            repository, branch, availability: if unavailable_reason.is_some() { TargetAvailability::Unavailable } else { TargetAvailability::Available }, unavailable_reason,
            agent_type: folder.default_agent_type, updated_at: observed_at });
    }
    Ok(targets)
}

fn target_prefix(runner_id: &str) -> String { format!("dextra-target:{runner_id}:folder:") }

pub(crate) fn target_id(runner_id: &str, folder_id: i32) -> String { format!("{}{folder_id}", target_prefix(runner_id)) }

pub(crate) async fn resolve_folder_reference(conn: &DatabaseConnection, runner_id: &str, reference: &str) -> Result<folder::Model, DbError> {
    let id = reference.strip_prefix(&target_prefix(runner_id)).and_then(|id| id.parse::<i32>().ok()).ok_or_else(|| DbError::NotFound(format!("客户端目录 {reference}")))?;
    folder::Entity::find_by_id(id).filter(folder::Column::DeletedAt.is_null()).filter(folder::Column::Kind.eq(FolderKind::Regular)).filter(folder::Column::ParentId.is_null()).one(conn).await?
        .ok_or_else(|| DbError::NotFound(format!("客户端目录 {reference}")))
}

fn availability_reason(folder: &folder::Model, path: &Path) -> Option<TargetProjectionReason> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {},
        Ok(_) => return Some(TargetProjectionReason { code: TargetProjectionReasonCode::WorkspacePathUnavailable, message: "客户端路径不是文件夹".into() }),
        Err(error) => return Some(TargetProjectionReason { code: if error.kind() == std::io::ErrorKind::NotFound { TargetProjectionReasonCode::WorkspacePathMissing } else { TargetProjectionReasonCode::WorkspacePathUnavailable }, message: error.to_string() }),
    }
    (!folder.is_open).then(|| TargetProjectionReason { code: TargetProjectionReasonCode::FolderClosed, message: "文件夹已在客户端关闭".into() })
}

async fn git_display(path: &Path) -> (Option<String>, Option<String>) {
    // Git 仅提供展示信息；普通目录和仓库子目录不会因此失去会话能力。
    let Ok(head) = resolve_git_head(&path.to_string_lossy()).await else { return (None, None); };
    if !head.is_repo { return (None, None); }
    let repository = match crate::process::tokio_command("git").args(["rev-parse", "--show-toplevel"]).current_dir(path).output().await {
        Ok(output) if output.status.success() => Path::new(String::from_utf8_lossy(&output.stdout).trim()).file_name().map(|name| name.to_string_lossy().into_owned()),
        _ => None,
    };
    (repository, head.branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};
    use sea_orm::{ActiveModelTrait, Set};

    #[tokio::test]
    async fn plain_folder_and_git_subdirectory_keep_their_selected_directory() {
        let temp = tempfile::tempdir().unwrap();
        let plain = temp.path().join("plain");
        let repo = temp.path().join("repo");
        let subdir = repo.join("src");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::create_dir_all(&subdir).unwrap();
        assert!(std::process::Command::new("git").args(["init", "-b", "main"]).current_dir(&repo).output().unwrap().status.success());
        let db = fresh_in_memory_db().await;
        let plain_id = seed_folder(&db, plain.to_str().unwrap()).await;
        let subdir_id = seed_folder(&db, subdir.to_str().unwrap()).await;
        for (id, path) in [(plain_id, plain), (subdir_id, subdir)] {
            let reference = target_id("client", id);
            assert_eq!(resolve_folder_reference(&db.conn, "client", &reference).await.unwrap().path, path.to_string_lossy());
            assert!(resolve_folder_reference(&db.conn, "another-client", &reference).await.is_err());
        }
        let targets = project_folder_targets(&db.conn, "client").await.unwrap();
        assert!(targets.iter().all(|target| target.availability == TargetAvailability::Available));
    }

    #[tokio::test]
    async fn closed_folder_remains_configurable_and_uses_its_ordinary_agent() {
        let temp = tempfile::tempdir().unwrap();
        let db = fresh_in_memory_db().await;
        let id = seed_folder(&db, temp.path().to_str().unwrap()).await;
        let row = folder::Entity::find_by_id(id).one(&db.conn).await.unwrap().unwrap();
        let mut active: folder::ActiveModel = row.into();
        active.is_open = Set(false);
        active.default_agent_type = Set(Some("codex".into()));
        active.update(&db.conn).await.unwrap();
        let targets = project_folder_targets(&db.conn, "client").await.unwrap();
        assert_eq!(targets[0].agent_type.as_deref(), Some("codex"));
        assert_eq!(targets[0].unavailable_reason.as_ref().unwrap().code, TargetProjectionReasonCode::FolderClosed);
        assert_eq!(resolve_folder_reference(&db.conn, "client", &targets[0].target_id).await.unwrap().id, id);
    }
}
