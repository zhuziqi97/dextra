//! 客户端目录事实投影；执行模块和 MCP 范围由服务端配置拥有。

use crate::commands::folders::resolve_git_head;
use crate::db::entities::folder::{self, FolderKind};
use crate::db::error::DbError;
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct FolderTargetProjection {
    pub target_id: String,
    pub native_project_kind: String,
    pub native_project_id: String,
    pub native_workspace_key: String,
    pub execution_host_id: String,
    pub path: Option<String>,
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
    LocalExecutionUnsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct FolderProjectProjection {
    pub native_project_kind: String,
    pub native_project_id: String,
    pub logical_project_id: Option<String>,
    pub display_name: String,
    pub execution_host_id: String,
    pub availability: TargetAvailability,
    pub unavailable_reason: Option<TargetProjectionReason>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct ProjectCatalog {
    pub projects: Vec<FolderProjectProjection>,
    pub targets: Vec<FolderTargetProjection>,
}

pub async fn project_folder_catalog(
    conn: &DatabaseConnection,
    runner_id: &str,
) -> Result<ProjectCatalog, DbError> {
    let folders = folder::Entity::find()
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .order_by_asc(folder::Column::Id)
        .all(conn)
        .await?;
    let observed_at = Utc::now();
    let mut projects = Vec::new();
    for root in folders.iter().filter(|row| row.parent_id.is_none()) {
        let reason = path_reason(Path::new(&root.path));
        projects.push(FolderProjectProjection {
            native_project_kind: "dextra_folder".into(),
            native_project_id: root.id.to_string(),
            logical_project_id: None,
            display_name: root.alias.clone().unwrap_or_else(|| root.name.clone()),
            execution_host_id: "local".into(),
            availability: if reason.is_some() {
                TargetAvailability::Unavailable
            } else {
                TargetAvailability::Available
            },
            unavailable_reason: reason,
            updated_at: observed_at,
        });
    }
    let mut targets = Vec::with_capacity(folders.len());
    for row in &folders {
        let root_id = row.parent_id.unwrap_or(row.id);
        let project = projects
            .iter()
            .find(|project| project.native_project_id == root_id.to_string())
            .ok_or_else(|| DbError::NotFound(format!("目录 {} 的根项目 {root_id}", row.id)))?;
        let path = Path::new(&row.path);
        let reason = project
            .unavailable_reason
            .clone()
            .or_else(|| availability_reason(row, path));
        let (repository, branch) = if reason.as_ref().is_some_and(|reason| {
            reason.code == TargetProjectionReasonCode::LocalExecutionUnsupported
        }) {
            (None, None)
        } else {
            git_display(path).await
        };
        targets.push(FolderTargetProjection {
            target_id: target_id(runner_id, row.id),
            native_project_kind: "dextra_folder".into(),
            native_project_id: root_id.to_string(),
            native_workspace_key: format!("folder:{}", row.id),
            execution_host_id: "local".into(),
            path: Some(row.path.clone()),
            display_name: row.alias.clone().unwrap_or_else(|| row.name.clone()),
            workspace_display_name: row.name.clone(),
            repository,
            branch,
            availability: if reason.is_some() {
                TargetAvailability::Unavailable
            } else {
                TargetAvailability::Available
            },
            unavailable_reason: reason,
            agent_type: row.default_agent_type.clone(),
            updated_at: observed_at,
        });
    }
    Ok(ProjectCatalog { projects, targets })
}

pub async fn project_root_for_folder(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<folder::Model, DbError> {
    let row = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("目录 {folder_id}")))?;
    match row.parent_id {
        None => Ok(row),
        Some(id) => folder::Entity::find_by_id(id)
            .filter(folder::Column::DeletedAt.is_null())
            .filter(folder::Column::Kind.eq(FolderKind::Regular))
            .filter(folder::Column::ParentId.is_null())
            .one(conn)
            .await?
            .ok_or_else(|| DbError::NotFound(format!("根项目 {id}"))),
    }
}

fn target_prefix(runner_id: &str) -> String {
    format!("dextra-target:{runner_id}:folder:")
}

pub(crate) fn target_id(runner_id: &str, folder_id: i32) -> String {
    format!("{}{folder_id}", target_prefix(runner_id))
}

pub(crate) async fn resolve_folder_reference(
    conn: &DatabaseConnection,
    runner_id: &str,
    reference: &str,
) -> Result<folder::Model, DbError> {
    let id = reference
        .strip_prefix(&target_prefix(runner_id))
        .and_then(|id| id.parse::<i32>().ok())
        .ok_or_else(|| DbError::NotFound(format!("客户端目录 {reference}")))?;
    folder::Entity::find_by_id(id)
        .filter(folder::Column::DeletedAt.is_null())
        .filter(folder::Column::Kind.eq(FolderKind::Regular))
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("客户端目录 {reference}")))
}

fn availability_reason(folder: &folder::Model, path: &Path) -> Option<TargetProjectionReason> {
    path_reason(path).or_else(|| {
        (!folder.is_open).then(|| TargetProjectionReason {
            code: TargetProjectionReasonCode::FolderClosed,
            message: "文件夹已在客户端关闭".into(),
        })
    })
}

fn path_reason(path: &Path) -> Option<TargetProjectionReason> {
    let text = path.to_string_lossy().to_ascii_lowercase();
    if text.starts_with(r"\\wsl.localhost\") || text.starts_with(r"\\wsl$\") {
        return Some(TargetProjectionReason {
            code: TargetProjectionReasonCode::LocalExecutionUnsupported,
            message: "Convene 当前仅支持已验证的本机目录，WSL 目录尚未支持".into(),
        });
    }
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Some(TargetProjectionReason {
                code: TargetProjectionReasonCode::WorkspacePathUnavailable,
                message: "客户端路径不是文件夹".into(),
            })
        }
        Err(error) => {
            return Some(TargetProjectionReason {
                code: if error.kind() == std::io::ErrorKind::NotFound {
                    TargetProjectionReasonCode::WorkspacePathMissing
                } else {
                    TargetProjectionReasonCode::WorkspacePathUnavailable
                },
                message: error.to_string(),
            })
        }
    }
    None
}

async fn git_display(path: &Path) -> (Option<String>, Option<String>) {
    // Git 仅提供展示信息；普通目录和仓库子目录不会因此失去会话能力。
    let Ok(head) = resolve_git_head(&path.to_string_lossy()).await else {
        return (None, None);
    };
    if !head.is_repo {
        return (None, None);
    }
    let repository = match crate::process::tokio_command("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            Path::new(String::from_utf8_lossy(&output.stdout).trim())
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }
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
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo)
            .output()
            .unwrap()
            .status
            .success());
        let db = fresh_in_memory_db().await;
        let plain_id = seed_folder(&db, plain.to_str().unwrap()).await;
        let subdir_id = seed_folder(&db, subdir.to_str().unwrap()).await;
        for (id, path) in [(plain_id, plain), (subdir_id, subdir)] {
            let reference = target_id("client", id);
            assert_eq!(
                resolve_folder_reference(&db.conn, "client", &reference)
                    .await
                    .unwrap()
                    .path,
                path.to_string_lossy()
            );
            assert!(
                resolve_folder_reference(&db.conn, "another-client", &reference)
                    .await
                    .is_err()
            );
        }
        let targets = project_folder_catalog(&db.conn, "client")
            .await
            .unwrap()
            .targets;
        assert!(targets
            .iter()
            .all(|target| target.availability == TargetAvailability::Available));
    }

    #[tokio::test]
    async fn closed_folder_remains_configurable_and_uses_its_ordinary_agent() {
        let temp = tempfile::tempdir().unwrap();
        let db = fresh_in_memory_db().await;
        let id = seed_folder(&db, temp.path().to_str().unwrap()).await;
        let row = folder::Entity::find_by_id(id)
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        let mut active: folder::ActiveModel = row.into();
        active.is_open = Set(false);
        active.default_agent_type = Set(Some("codex".into()));
        active.update(&db.conn).await.unwrap();
        let targets = project_folder_catalog(&db.conn, "client")
            .await
            .unwrap()
            .targets;
        assert_eq!(targets[0].agent_type.as_deref(), Some("codex"));
        assert_eq!(
            targets[0].unavailable_reason.as_ref().unwrap().code,
            TargetProjectionReasonCode::FolderClosed
        );
        assert_eq!(
            resolve_folder_reference(&db.conn, "client", &targets[0].target_id)
                .await
                .unwrap()
                .id,
            id
        );
    }
    #[tokio::test]
    async fn wsl_folder_is_retained_without_becoming_a_local_execution_candidate() {
        let db = fresh_in_memory_db().await;
        let id = seed_folder(&db, r"\\wsl.localhost\Ubuntu\home\user\repo").await;
        let catalog = project_folder_catalog(&db.conn, "device").await.unwrap();
        assert_eq!(catalog.projects[0].native_project_id, id.to_string());
        assert_eq!(
            catalog.targets[0].availability,
            TargetAvailability::Unavailable
        );
        assert_eq!(
            catalog.targets[0].unavailable_reason.as_ref().unwrap().code,
            TargetProjectionReasonCode::LocalExecutionUnsupported
        );
    }

    #[tokio::test]
    async fn two_worktree_folders_share_the_native_root_but_keep_distinct_targets() {
        let temp = tempfile::tempdir().unwrap();
        let db = fresh_in_memory_db().await;
        let root = seed_folder(&db, temp.path().to_str().unwrap()).await;
        let mut children = Vec::new();
        for name in ["one", "two"] {
            let path = temp.path().join(name);
            std::fs::create_dir(&path).unwrap();
            let id = seed_folder(&db, path.to_str().unwrap()).await;
            let row = folder::Entity::find_by_id(id)
                .one(&db.conn)
                .await
                .unwrap()
                .unwrap();
            let mut active: folder::ActiveModel = row.into();
            active.parent_id = Set(Some(root));
            active.update(&db.conn).await.unwrap();
            children.push(id);
        }
        let catalog = project_folder_catalog(&db.conn, "device").await.unwrap();
        assert_eq!(catalog.projects.len(), 1);
        assert_eq!(catalog.targets.len(), 3);
        for id in children {
            let target = catalog
                .targets
                .iter()
                .find(|row| row.native_workspace_key == format!("folder:{id}"))
                .unwrap();
            assert_eq!(target.native_project_id, root.to_string());
            assert_eq!(
                resolve_folder_reference(&db.conn, "device", &target.target_id)
                    .await
                    .unwrap()
                    .id,
                id
            );
            assert_eq!(
                project_root_for_folder(&db.conn, id).await.unwrap().id,
                root
            );
        }
        let another = temp.path().join("independent");
        std::fs::create_dir(&another).unwrap();
        let independent = seed_folder(&db, another.to_str().unwrap()).await;
        let catalog = project_folder_catalog(&db.conn, "device").await.unwrap();
        assert_eq!(catalog.projects.len(), 2);
        assert_eq!(
            catalog
                .targets
                .iter()
                .find(|row| row.native_workspace_key == format!("folder:{independent}"))
                .unwrap()
                .native_project_id,
            independent.to_string()
        );
    }
}
