//! 普通会话与 WorkTask 共用的目录模块选择；只保存作用域引用。

use super::{identity, identity::CerebroTargetBinding, target_projection};
use crate::app_error::{AppCommandError, AppErrorCode};
use crate::db::{
    entities::{cerebro_task_link, conversation, folder, work_task},
    error::DbError,
};
use sea_orm::{sea_query::Expr, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(tag = "mode")]
pub enum CerebroSelection {
    #[serde(rename = "LOCAL")]
    Local,
    #[serde(rename = "BINDING")]
    Binding { binding_id: String },
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct CerebroLaunchBinding {
    pub binding: Option<CerebroTargetBinding>,
    pub selection: Option<CerebroSelection>,
    pub platform_task: bool,
    pub binding_query_error: Option<String>,
}

/// 已有入口可能只传 provider session ID；仍须恢复同一会话保存的选择。
pub async fn resolve_conversation_id(
    conn: &DatabaseConnection,
    agent_type: crate::models::AgentType,
    session_id: Option<&str>,
    requested: Option<i32>,
) -> Result<Option<i32>, AppCommandError> {
    if requested.is_some() { return Ok(requested); }
    let Some(session_id) = session_id else { return Ok(None); };
    Ok(conversation::Entity::find()
        .filter(conversation::Column::ExternalId.eq(session_id))
        .filter(conversation::Column::AgentType.eq(agent_type.as_wire().into_owned()))
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn).await.map_err(DbError::from)?.map(|row| row.id))
}

pub async fn query_launch_binding(
    conn: &DatabaseConnection,
    folder_id: i32,
    conversation_id: Option<i32>,
    work_task_id: Option<i32>,
) -> Result<CerebroLaunchBinding, AppCommandError> {
    let (platform_task, selection) = if let Some(id) = work_task_id {
        let task = crate::db::service::work_task_service::get_model(conn, id).await?;
        let config: crate::models::WorkTaskConfig = serde_json::from_str(&task.config)
            .map_err(|error| AppCommandError::invalid_input(error.to_string()))?;
        (platform_task_for_work_task(conn, id).await?.is_some(),
            if folder_id == task.folder_id { config.cerebro_selection } else { None })
    } else if let Some(id) = conversation_id {
        (platform_task_for_conversation(conn, id).await?.is_some(), load_conversation_selection(conn, id).await?)
    } else { (false, None) };
    let lookup = if platform_task { Ok(None) } else { query_folder_binding(conn, folder_id).await };
    launch_binding_context(platform_task, selection, lookup)
}

fn launch_binding_context(
    platform_task: bool,
    selection: Option<CerebroSelection>,
    lookup: Result<Option<CerebroTargetBinding>, AppCommandError>,
) -> Result<CerebroLaunchBinding, AppCommandError> {
    let (binding, binding_query_error) = match lookup {
        Ok(binding) => (binding, None),
        // 已明确保存纯本地时，模块查询只影响选择展示，不影响本地启动。
        Err(error) if selection == Some(CerebroSelection::Local) => (None, Some(error.to_string())),
        Err(error) => return Err(error),
    };
    Ok(CerebroLaunchBinding { binding, selection, platform_task, binding_query_error })
}

pub async fn prepare_work_task_draft(
    conn: &DatabaseConnection,
    mut draft: crate::models::WorkTaskDraft,
) -> Result<crate::models::WorkTaskDraft, AppCommandError> {
    let config: crate::models::WorkTaskConfig = serde_json::from_value(draft.config.clone())
        .map_err(|error| AppCommandError::invalid_input(error.to_string()))?;
    let selection =
        resolve_folder_selection(conn, draft.folder_id, config.cerebro_selection.as_ref()).await?;
    // 只写本适配层拥有的字段，不丢弃任务配置的其它内容。
    let object = draft
        .config
        .as_object_mut()
        .ok_or_else(|| AppCommandError::invalid_input("任务配置必须是对象"))?;
    object.insert(
        "cerebro_selection".into(),
        serde_json::to_value(selection)
            .map_err(|error| AppCommandError::invalid_input(error.to_string()))?,
    );
    Ok(draft)
}

pub async fn platform_task_for_conversation(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<Option<String>, AppCommandError> {
    let task = work_task::Entity::find()
        .filter(work_task::Column::ConversationId.eq(conversation_id))
        .one(conn)
        .await
        .map_err(DbError::from)?;
    let Some(task) = task else {
        return Ok(None);
    };
    platform_task_for_work_task(conn, task.id).await
}

pub async fn platform_task_for_work_task(
    conn: &DatabaseConnection,
    task_id: i32,
) -> Result<Option<String>, AppCommandError> {
    Ok(cerebro_task_link::Entity::find()
        .filter(cerebro_task_link::Column::LocalWorkTaskId.eq(task_id))
        .one(conn)
        .await
        .map_err(DbError::from)?
        .map(|link| link.platform_task_id))
}

/// 从已保存会话或实际目录取得新连接的选择；旧会话空值才使用目录默认。
pub async fn selection_for_start(
    conn: &DatabaseConnection,
    working_dir: Option<&str>,
    conversation_id: Option<i32>,
    selected: Option<&CerebroSelection>,
) -> Result<CerebroSelection, AppCommandError> {
    let (folder_id, saved) = if let Some(id) = conversation_id {
        let row = conversation::Entity::find_by_id(id)
            .filter(conversation::Column::DeletedAt.is_null())
            .one(conn)
            .await
            .map_err(DbError::from)?
            .ok_or_else(|| AppCommandError::new(AppErrorCode::NotFound, "会话不存在"))?;
        (
            Some(row.folder_id),
            load_conversation_selection(conn, id).await?,
        )
    } else if let Some(path) = working_dir {
        let row = folder::Entity::find()
            .filter(folder::Column::Path.eq(path))
            .filter(folder::Column::DeletedAt.is_null())
            .one(conn)
            .await
            .map_err(DbError::from)?;
        (row.map(|row| row.id), None)
    } else {
        (None, None)
    };
    let selected = selected.or(saved.as_ref());
    match folder_id {
        Some(id) => resolve_folder_selection(conn, id, selected).await,
        None => resolve_selection(selected, None),
    }
}

pub async fn load_conversation_selection(
    conn: &DatabaseConnection,
    conversation_id: i32,
) -> Result<Option<CerebroSelection>, AppCommandError> {
    let row = conversation::Entity::find_by_id(conversation_id)
        .filter(conversation::Column::DeletedAt.is_null())
        .one(conn)
        .await
        .map_err(DbError::from)?
        .ok_or_else(|| AppCommandError::new(AppErrorCode::NotFound, "会话不存在"))?;
    row.cerebro_selection
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                AppCommandError::new(AppErrorCode::ConfigurationInvalid, error.to_string())
            })
        })
        .transpose()
}

pub async fn save_conversation_selection(
    conn: &DatabaseConnection,
    conversation_id: i32,
    selection: &CerebroSelection,
) -> Result<(), AppCommandError> {
    let value = serde_json::to_string(selection).map_err(|error| {
        AppCommandError::new(AppErrorCode::ConfigurationInvalid, error.to_string())
    })?;
    let result = conversation::Entity::update_many()
        .col_expr(conversation::Column::CerebroSelection, Expr::value(value))
        .filter(conversation::Column::Id.eq(conversation_id))
        .filter(conversation::Column::DeletedAt.is_null())
        .exec(conn)
        .await
        .map_err(DbError::from)?;
    if result.rows_affected == 0 {
        return Err(AppCommandError::new(AppErrorCode::NotFound, "会话不存在"));
    }
    Ok(())
}

/// 已有会话立即保存；新会话复用已有关联事件保存本次启动选择。
pub async fn persist_launch_selection(
    conn: &DatabaseConnection,
    manager: &crate::acp::manager::ConnectionManager,
    connection_id: &str,
    conversation_id: Option<i32>,
    selection: CerebroSelection,
) -> Result<(), AppCommandError> {
    let (state, emitter) = manager.get_state_and_emitter(connection_id).await
        .ok_or_else(|| AppCommandError::task_execution_failed("连接在启动后已退出"))?;
    let (linked, events, agent_type) = {
        let state = state.read().await;
        (state.conversation_id.or(conversation_id), state.event_stream.subscribe(), state.agent_type)
    };
    if let Some(id) = linked {
        return save_conversation_selection(conn, id, &selection).await;
    }
    let conn = conn.clone();
    tokio::spawn(async move {
        if let Err(error) = persist_selection_from_events(&conn, events, &selection).await {
            crate::web::event_bridge::emit_with_state(
                &state, &emitter, crate::acp::types::AcpEvent::Error {
                    message: error.to_string(), agent_type: agent_type.as_wire().into_owned(),
                    code: Some("cerebro_selection_save_failed".into()),
                    details: None, terminal: false,
                },
            ).await;
        }
    });
    Ok(())
}

async fn persist_selection_from_events(
    conn: &DatabaseConnection,
    mut events: tokio::sync::broadcast::Receiver<std::sync::Arc<crate::acp::types::EventEnvelope>>,
    selection: &CerebroSelection,
) -> Result<(), AppCommandError> {
    use crate::acp::types::{AcpEvent, ConnectionStatus};
    loop {
        match events.recv().await {
            Ok(event) => match &event.payload {
                AcpEvent::ConversationLinked { conversation_id, .. } => {
                    return save_conversation_selection(conn, *conversation_id, selection).await;
                }
                AcpEvent::StatusChanged { status: ConnectionStatus::Disconnected, .. } => return Ok(()),
                _ => {},
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
            Err(error) => return Err(AppCommandError::task_execution_failed(error.to_string())),
        }
    }
}

/// 从真实 Folder 关联解析 Target，不按物理路径跨设备去重。
pub async fn query_folder_binding(
    conn: &DatabaseConnection,
    folder_id: i32,
) -> Result<Option<CerebroTargetBinding>, AppCommandError> {
    let folder = folder::Entity::find_by_id(folder_id)
        .filter(folder::Column::DeletedAt.is_null())
        .one(conn)
        .await
        .map_err(DbError::from)?
        .ok_or_else(|| AppCommandError::new(AppErrorCode::NotFound, "目录不存在"))?;
    if folder.kind == folder::FolderKind::Chat {
        return Ok(None);
    }
    let auth = identity::get_auth_state().await?;
    let Some(runner_id) = auth.runner_id else {
        return Ok(None);
    };
    identity::query_target_binding(&target_projection::target_id(
        &runner_id,
        folder.parent_id.unwrap_or(folder.id),
    ))
    .await
}

/// 查询错误由调用方原样传播；这里只解析成功取得的当前业务事实。
pub fn resolve_selection(
    selected: Option<&CerebroSelection>,
    binding: Option<&CerebroTargetBinding>,
) -> Result<CerebroSelection, AppCommandError> {
    if selected == Some(&CerebroSelection::Local) {
        return Ok(CerebroSelection::Local);
    }
    if let Some(CerebroSelection::Binding { binding_id }) = selected {
        if binding.is_none_or(|current| &current.binding_id != binding_id) {
            return Err(AppCommandError::new(
                AppErrorCode::ConfigurationInvalid,
                "已选择的 Cerebro Binding 不再属于当前目录；请选择后续操作",
            ));
        }
    }
    let Some(binding) = binding else {
        return Ok(CerebroSelection::Local);
    };
    if let Some(code) = binding.unavailable_code.as_ref() {
        let mut error = AppCommandError::new(
            AppErrorCode::ConfigurationInvalid,
            binding.unavailable_message.as_deref().unwrap_or(code),
        );
        error.detail = Some(code.clone());
        return Err(error);
    }
    Ok(CerebroSelection::Binding {
        binding_id: binding.binding_id.clone(),
    })
}

pub async fn resolve_folder_selection(
    conn: &DatabaseConnection,
    folder_id: i32,
    selected: Option<&CerebroSelection>,
) -> Result<CerebroSelection, AppCommandError> {
    if selected == Some(&CerebroSelection::Local) {
        return Ok(CerebroSelection::Local);
    }
    let binding = query_folder_binding(conn, folder_id).await?;
    resolve_selection(selected, binding.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_local_remains_ready_without_hiding_binding_query_failure() {
        let error = || AppCommandError::new(AppErrorCode::ConfigurationInvalid, "Cerebro 连接被拒绝");
        let local = launch_binding_context(false, Some(CerebroSelection::Local), Err(error())).unwrap();
        assert_eq!(local.selection, Some(CerebroSelection::Local));
        assert!(local.binding.is_none());
        assert!(local.binding_query_error.unwrap().contains("Cerebro 连接被拒绝"));
        for selection in [None, Some(CerebroSelection::Binding { binding_id: "saved".into() })] {
            assert_eq!(launch_binding_context(false, selection, Err(error())).unwrap_err().message,
                "Cerebro 连接被拒绝");
        }
    }

    #[tokio::test]
    async fn normal_task_creation_saves_explicit_local_without_credentials() {
        use crate::db::test_helpers::{fresh_in_memory_db, seed_folder};
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-local-task").await;
        let emitter = crate::web::event_bridge::EventEmitter::test_web_only(std::sync::Arc::new(
            crate::web::event_bridge::WebEventBroadcaster::new(),
        ));
        let task = crate::commands::work_task::work_task_create_core(&emitter, &db,
            crate::models::WorkTaskDraft {
                folder_id,
                title: "本地任务".into(),
                config: serde_json::json!({"display_text":"读取本地代码", "cerebro_selection":{"mode":"LOCAL"}}),
            },
        ).await.unwrap();
        let row = crate::db::service::work_task_service::get_model(&db.conn, task.id)
            .await
            .unwrap();
        let config: crate::models::WorkTaskConfig = serde_json::from_str(&row.config).unwrap();
        assert_eq!(config.cerebro_selection, Some(CerebroSelection::Local));
    }

    #[tokio::test]
    async fn first_conversation_link_persists_the_launch_choice() {
        use crate::acp::types::{AcpEvent, EventEnvelope};
        use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/selection-link-event").await;
        let id = seed_conversation(&db, folder_id, crate::models::AgentType::Codex).await;
        let (sender, receiver) = tokio::sync::broadcast::channel(8);
        let selection = CerebroSelection::Binding { binding_id: "chosen-binding".into() };
        sender.send(std::sync::Arc::new(EventEnvelope {
            seq: 1, connection_id: "new-chat".into(),
            payload: AcpEvent::ConversationLinked {
                conversation_id: id, folder_id,
                parent_conversation_id: None, parent_tool_use_id: None,
            },
        })).unwrap();
        persist_selection_from_events(&db.conn, receiver, &selection).await.unwrap();
        assert_eq!(load_conversation_selection(&db.conn, id).await.unwrap(), Some(selection));
    }

    #[tokio::test]
    async fn persisted_selection_survives_reopen_and_invalid_data_is_not_local() {
        use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-selection").await;
        let id = seed_conversation(&db, folder_id, crate::models::AgentType::Codex).await;
        assert_eq!(
            load_conversation_selection(&db.conn, id).await.unwrap(),
            None
        );
        let selection = CerebroSelection::Binding {
            binding_id: "saved-binding".into(),
        };
        save_conversation_selection(&db.conn, id, &selection)
            .await
            .unwrap();
        assert_eq!(
            load_conversation_selection(&db.conn, id).await.unwrap(),
            Some(selection)
        );
        save_conversation_selection(&db.conn, id, &CerebroSelection::Local)
            .await
            .unwrap();
        assert_eq!(
            load_conversation_selection(&db.conn, id).await.unwrap(),
            Some(CerebroSelection::Local)
        );
        conversation::Entity::update_many()
            .col_expr(
                conversation::Column::CerebroSelection,
                Expr::value("broken-json"),
            )
            .filter(conversation::Column::Id.eq(id))
            .exec(&db.conn)
            .await
            .unwrap();
        assert!(load_conversation_selection(&db.conn, id)
            .await
            .unwrap_err()
            .message
            .contains("expected value"));
    }

    fn available() -> CerebroTargetBinding {
        CerebroTargetBinding {
            binding_id: "binding-one".into(),
            module_path: "owner/project/module".into(),
            module_display_name: "模块".into(),
            status: "ACTIVE".into(),
            unavailable_code: None,
            unavailable_message: None,
        }
    }

    #[test]
    fn new_session_defaults_to_unique_binding_but_allows_local() {
        assert_eq!(
            resolve_selection(None, None).unwrap(),
            CerebroSelection::Local
        );
        let binding = available();
        assert_eq!(
            resolve_selection(None, Some(&binding)).unwrap(),
            CerebroSelection::Binding {
                binding_id: binding.binding_id.clone(),
            }
        );
        assert_eq!(
            resolve_selection(Some(&CerebroSelection::Local), Some(&binding)).unwrap(),
            CerebroSelection::Local
        );
    }

    #[test]
    fn disabled_binding_keeps_real_reason_and_requires_explicit_local() {
        let mut binding = available();
        binding.status = "DISABLED".into();
        binding.unavailable_code = Some("BINDING_NOT_ACTIVE".into());
        binding.unavailable_message = Some("模块绑定已停用".into());
        let error = resolve_selection(None, Some(&binding)).unwrap_err();
        assert_eq!(error.message, "模块绑定已停用");
        assert_eq!(error.detail.as_deref(), Some("BINDING_NOT_ACTIVE"));
        assert_eq!(
            resolve_selection(Some(&CerebroSelection::Local), Some(&binding)).unwrap(),
            CerebroSelection::Local
        );
    }

    #[test]
    fn saved_binding_never_switches_to_replacement_or_local() {
        let saved = CerebroSelection::Binding {
            binding_id: "revoked-binding".into(),
        };
        assert!(resolve_selection(Some(&saved), None).is_err());
        assert!(resolve_selection(Some(&saved), Some(&available())).is_err());
    }

    #[test]
    fn selection_round_trip_only_retains_mode_and_reference() {
        for selection in [
            CerebroSelection::Local,
            CerebroSelection::Binding {
                binding_id: "saved".into(),
            },
        ] {
            let value = serde_json::to_value(&selection).unwrap();
            assert_eq!(
                serde_json::from_value::<CerebroSelection>(value).unwrap(),
                selection
            );
        }
    }
}
