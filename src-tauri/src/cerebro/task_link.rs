use std::sync::Arc;

use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::commands::work_task::nudge_pump;
use crate::db::entities::work_task::WorkTaskStatus;
use crate::db::entities::{cerebro_task_link, work_task};
use crate::db::error::DbError;
use crate::db::service::work_task_service;
use crate::models::{WorkTaskDraft, WorkTaskInfo};
use crate::web::event_bridge::{emit_event, EventEmitter, WorkTaskChange, WORK_TASK_CHANGED_EVENT};
use crate::work_task::TaskEngine;

#[derive(Debug)]
pub enum LinkedWorkTaskCreateOutcome {
    Created(WorkTaskInfo),
    Duplicate(WorkTaskInfo),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWorkTaskCancelReceipt {
    pub local_work_task_id: i32,
    pub run_seq: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkedWorkTaskCancelOutcome {
    Accepted(LinkedWorkTaskCancelReceipt),
    Duplicate { local_work_task_id: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkedWorkTaskState {
    Queued,
    Running,
    WaitingForUser,
    ResultReady,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct LinkedWorkTaskSnapshot {
    pub platform_task_id: String,
    pub local_work_task_id: i32,
    pub state: LinkedWorkTaskState,
    pub failure_reason: Option<String>,
    pub last_error: Option<String>,
    pub result_summary: Option<String>,
    pub remote_workbench_available: bool,
    pub deleted: bool,
}

/// 把 Cerebro 平台 Task 原子关联到一个原生 WorkTask。
///
/// tentative WorkTask 是事务第一条写入，因此并发调用会在 SQLite 写锁处串行。
/// 取得写锁后再查永久 link；重复请求回滚 tentative WorkTask 和 created event，
/// 只返回第一次创建的 WorkTask。事件和 pump nudge 仅在首次提交成功后发生。
pub async fn create_linked_work_task(
    emitter: &EventEmitter,
    conn: &DatabaseConnection,
    platform_task_id: &str,
    draft: WorkTaskDraft,
) -> Result<LinkedWorkTaskCreateOutcome, DbError> {
    if platform_task_id.trim().is_empty() {
        return Err(DbError::Validation("platform task id is required".into()));
    }
    let prepared = work_task_service::prepare_create(conn, draft).await?;
    let txn = conn.begin().await?;

    // 第一条事务语句必须是写入；不能把 link 查询移到它前面。
    let tentative = work_task_service::insert_prepared_with_created_event(&txn, &prepared).await?;
    if let Some(link) = cerebro_task_link::Entity::find_by_id(platform_task_id)
        .one(&txn)
        .await?
    {
        let existing = work_task::Entity::find_by_id(link.local_work_task_id)
            .one(&txn)
            .await?
            .ok_or_else(|| {
                DbError::NotFound(format!(
                    "linked work task {} for platform task {}",
                    link.local_work_task_id, platform_task_id
                ))
            })?;
        txn.rollback().await?;
        return Ok(LinkedWorkTaskCreateOutcome::Duplicate(
            work_task_service::to_info(existing),
        ));
    }

    cerebro_task_link::ActiveModel {
        platform_task_id: Set(platform_task_id.to_string()),
        local_work_task_id: Set(tentative.id),
        cancel_command_id: Set(None),
        created_at: Set(Utc::now()),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    let info = work_task_service::to_info(tentative);
    emit_event(
        emitter,
        WORK_TASK_CHANGED_EVENT,
        WorkTaskChange::Upsert { id: info.id },
    );
    nudge_pump(info.folder_id);
    Ok(LinkedWorkTaskCreateOutcome::Created(info))
}

#[derive(Debug)]
pub(crate) struct CommittedLinkedCancel {
    pub(crate) outcome: LinkedWorkTaskCancelOutcome,
    pub(crate) context: Option<work_task_service::CanceledExecutionContext>,
}

/// 原子提交 linked WorkTask 取消和最小 command receipt。
pub(crate) async fn commit_linked_cancel(
    conn: &DatabaseConnection,
    platform_task_id: &str,
    command_id: &str,
    reason: Option<&str>,
) -> Result<CommittedLinkedCancel, DbError> {
    if platform_task_id.trim().is_empty() {
        return Err(DbError::Validation("platform task id is required".into()));
    }
    if command_id.trim().is_empty() {
        return Err(DbError::Validation("cancel command id is required".into()));
    }

    let txn = conn.begin().await?;
    // receipt UPDATE 是第一条事务语句，先取得 SQLite 写锁，再判断重复或取消状态。
    let receipt_write = cerebro_task_link::Entity::update_many()
        .col_expr(
            cerebro_task_link::Column::CancelCommandId,
            Expr::value(Some(command_id.to_string())),
        )
        .filter(cerebro_task_link::Column::PlatformTaskId.eq(platform_task_id))
        .filter(cerebro_task_link::Column::CancelCommandId.is_null())
        .exec(&txn)
        .await?;
    let link = cerebro_task_link::Entity::find_by_id(platform_task_id)
        .one(&txn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("platform task {platform_task_id}")))?;

    if receipt_write.rows_affected == 0 {
        if link.cancel_command_id.as_deref() == Some(command_id) {
            txn.rollback().await?;
            return Ok(CommittedLinkedCancel {
                outcome: LinkedWorkTaskCancelOutcome::Duplicate {
                    local_work_task_id: link.local_work_task_id,
                },
                context: None,
            });
        }
        txn.rollback().await?;
        return Err(DbError::Validation(format!(
            "platform task {platform_task_id} already has another cancel command"
        )));
    }

    let task = work_task::Entity::find_by_id(link.local_work_task_id)
        .one(&txn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("work task {}", link.local_work_task_id)))?;
    let context = if task.deleted_at.is_none() && task.status == WorkTaskStatus::Canceled {
        None
    } else {
        work_task_service::cancel_in_transaction(
            &txn,
            task.id,
            &[
                WorkTaskStatus::Todo,
                WorkTaskStatus::Queued,
                WorkTaskStatus::Preparing,
                WorkTaskStatus::Running,
                WorkTaskStatus::AwaitingInput,
                WorkTaskStatus::Review,
                WorkTaskStatus::Failed,
            ],
            None,
            "user",
            reason,
        )
        .await?
    };
    if context.is_none() && task.status != WorkTaskStatus::Canceled {
        txn.rollback().await?;
        return Err(DbError::Validation(
            "linked work task cannot be canceled in its current state".into(),
        ));
    }

    let receipt = LinkedWorkTaskCancelReceipt {
        local_work_task_id: task.id,
        run_seq: context
            .as_ref()
            .map_or(task.run_seq, |context| context.run_seq),
    };
    txn.commit().await?;
    Ok(CommittedLinkedCancel {
        outcome: LinkedWorkTaskCancelOutcome::Accepted(receipt),
        context,
    })
}

/// 提交成功后清理被冻结的执行代次，再把 ACK outcome 返回给协议 adapter。
pub async fn cancel_linked_work_task(
    engine: &Arc<TaskEngine>,
    emitter: &EventEmitter,
    conn: &DatabaseConnection,
    platform_task_id: &str,
    command_id: &str,
    reason: Option<&str>,
) -> Result<LinkedWorkTaskCancelOutcome, DbError> {
    let committed = commit_linked_cancel(conn, platform_task_id, command_id, reason).await?;
    if let Some(context) = committed.context.as_ref() {
        emit_event(
            emitter,
            WORK_TASK_CHANGED_EVENT,
            WorkTaskChange::Upsert {
                id: context.task_id,
            },
        );
        engine.cleanup_canceled_execution(context).await;
    }
    Ok(committed.outcome)
}

/// 从永久 link、cancel receipt 和包含 soft-deleted row 的内部读取重建平台状态。
pub async fn reconcile_linked_work_task(
    conn: &DatabaseConnection,
    platform_task_id: &str,
) -> Result<LinkedWorkTaskSnapshot, DbError> {
    let link = cerebro_task_link::Entity::find_by_id(platform_task_id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("platform task {platform_task_id}")))?;
    let task = work_task::Entity::find_by_id(link.local_work_task_id)
        .one(conn)
        .await?
        .ok_or_else(|| DbError::NotFound(format!("work task {}", link.local_work_task_id)))?;
    let receipt_exists = link.cancel_command_id.is_some();
    let deleted = task.deleted_at.is_some();
    let state = if receipt_exists {
        LinkedWorkTaskState::Cancelled
    } else if deleted {
        if task.status == WorkTaskStatus::Done {
            LinkedWorkTaskState::Completed
        } else {
            LinkedWorkTaskState::Cancelled
        }
    } else {
        match task.status {
            WorkTaskStatus::Todo | WorkTaskStatus::Queued | WorkTaskStatus::Preparing => {
                LinkedWorkTaskState::Queued
            }
            WorkTaskStatus::Running | WorkTaskStatus::Merging => LinkedWorkTaskState::Running,
            WorkTaskStatus::AwaitingInput => LinkedWorkTaskState::WaitingForUser,
            WorkTaskStatus::Review => LinkedWorkTaskState::ResultReady,
            WorkTaskStatus::Done => LinkedWorkTaskState::Completed,
            WorkTaskStatus::Failed => LinkedWorkTaskState::Failed,
            WorkTaskStatus::Canceled => LinkedWorkTaskState::Cancelled,
        }
    };

    Ok(LinkedWorkTaskSnapshot {
        platform_task_id: link.platform_task_id,
        local_work_task_id: task.id,
        state,
        failure_reason: task.failure_reason,
        last_error: task.last_error,
        result_summary: task.result_summary,
        remote_workbench_available: !receipt_exists
            && !deleted
            && task.status == WorkTaskStatus::Review,
        deleted,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sea_orm::{ActiveModelTrait, Database, EntityTrait, IntoActiveModel, PaginatorTrait};
    use sea_orm_migration::MigratorTrait;

    use super::*;
    use crate::db::entities::{cerebro_task_link, work_task, work_task_event};
    use crate::db::migration::Migrator;
    use crate::db::service::work_task_service;
    use crate::db::test_helpers::{
        fresh_disk_db, fresh_in_memory_db, seed_conversation, seed_folder,
    };
    use crate::models::agent::AgentType;
    use crate::web::event_bridge::{WebEventBroadcaster, WORK_TASK_CHANGED_EVENT};

    fn draft(folder_id: i32, title: &str) -> WorkTaskDraft {
        WorkTaskDraft {
            folder_id,
            title: title.to_string(),
            config: serde_json::json!({
                "display_text": "执行平台任务",
                "prompt_blocks": [{ "type": "text", "text": "执行平台任务" }],
            }),
        }
    }

    async fn counts(conn: &DatabaseConnection) -> (u64, u64, u64) {
        (
            work_task::Entity::find().count(conn).await.unwrap(),
            cerebro_task_link::Entity::find().count(conn).await.unwrap(),
            work_task_event::Entity::find().count(conn).await.unwrap(),
        )
    }

    #[tokio::test]
    async fn first_create_commits_once_and_duplicate_rolls_back_without_emitting() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-linked-create").await;
        let broadcaster = Arc::new(WebEventBroadcaster::new());
        let mut events = broadcaster.subscribe();
        let emitter = EventEmitter::test_web_only(broadcaster);

        let first = match create_linked_work_task(
            &emitter,
            &db.conn,
            "platform-task-1",
            draft(folder_id, "first"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Created(info) => info,
            other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
        };
        assert_eq!(counts(&db.conn).await, (1, 1, 1));
        assert_eq!(first.source_kind, None);
        assert_eq!(first.source_key, None);
        let link = cerebro_task_link::Entity::find_by_id("platform-task-1")
            .one(&db.conn)
            .await
            .unwrap()
            .expect("首次提交应写入永久 link");
        assert_eq!(link.local_work_task_id, first.id);
        assert_eq!(link.cancel_command_id, None);
        let event = events.try_recv().expect("首次提交后应发出 Upsert");
        assert_eq!(event.channel, WORK_TASK_CHANGED_EVENT);
        assert_eq!(event.payload["kind"], "upsert");
        assert_eq!(event.payload["id"], first.id);

        let repeated = match create_linked_work_task(
            &emitter,
            &db.conn,
            "platform-task-1",
            draft(folder_id, "tentative duplicate"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Duplicate(info) => info,
            other => panic!("重复调用应返回既有 WorkTask，实际为 {other:?}"),
        };
        assert_eq!(repeated.id, first.id);
        // tentative WorkTask 与 created event 必须随重复事务一起回滚。
        assert_eq!(counts(&db.conn).await, (1, 1, 1));
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        let plain = work_task_service::create(&db.conn, draft(folder_id, "manual"))
            .await
            .unwrap();
        assert_ne!(plain.id, first.id);
        assert_eq!(counts(&db.conn).await, (2, 1, 2));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_replay_yields_one_created_and_one_duplicate() {
        let dir = tempfile::tempdir().expect("创建临时目录");
        let db = fresh_disk_db(dir.path()).await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-linked-race").await;
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut handles = Vec::new();

        for n in 0..2 {
            let conn = db.conn.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                create_linked_work_task(
                    &EventEmitter::Noop,
                    &conn,
                    "platform-task-race",
                    draft(folder_id, &format!("racer-{n}")),
                )
                .await
            }));
        }

        let mut created = Vec::new();
        let mut duplicate = Vec::new();
        for handle in handles {
            match handle.await.expect("并发任务完成").expect("创建不得锁失败") {
                LinkedWorkTaskCreateOutcome::Created(info) => created.push(info.id),
                LinkedWorkTaskCreateOutcome::Duplicate(info) => duplicate.push(info.id),
            }
        }
        assert_eq!(created.len(), 1);
        assert_eq!(duplicate, created);
        assert_eq!(counts(&db.conn).await, (1, 1, 1));
    }

    #[tokio::test]
    async fn replay_after_database_reopen_returns_the_original_work_task() {
        let dir = tempfile::tempdir().expect("创建临时目录");
        let db = fresh_disk_db(dir.path()).await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-linked-reopen").await;
        let first = match create_linked_work_task(
            &EventEmitter::Noop,
            &db.conn,
            "platform-task-reopen",
            draft(folder_id, "before restart"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Created(info) => info,
            other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
        };
        db.conn.close().await.unwrap();

        let url = format!("sqlite:{}?mode=rwc", dir.path().join("source.db").display());
        let conn = Database::connect(url).await.expect("重新打开数据库");
        Migrator::up(&conn, None).await.expect("确认迁移已应用");
        let repeated = match create_linked_work_task(
            &EventEmitter::Noop,
            &conn,
            "platform-task-reopen",
            draft(folder_id, "after restart"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Duplicate(info) => info,
            other => panic!("重启后重放应命中既有关联，实际为 {other:?}"),
        };
        assert_eq!(repeated.id, first.id);
        assert_eq!(counts(&conn).await, (1, 1, 1));
    }

    #[tokio::test]
    async fn rejects_empty_platform_task_id_before_creating_any_row() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-linked-invalid").await;

        let error = create_linked_work_task(
            &EventEmitter::Noop,
            &db.conn,
            "   ",
            draft(folder_id, "invalid"),
        )
        .await
        .expect_err("空平台 Task ID 必须拒绝");
        assert!(error.to_string().contains("platform task id"));
        assert_eq!(counts(&db.conn).await, (0, 0, 0));
    }

    #[tokio::test]
    async fn lost_cancel_ack_replay_does_not_cancel_a_local_requeue() {
        let db = fresh_in_memory_db().await;
        let conn = db.conn.clone();
        let folder_id = seed_folder(&db, "/tmp/cerebro-cancel-replay").await;
        let task = match create_linked_work_task(
            &EventEmitter::Noop,
            &conn,
            "platform-cancel-replay",
            draft(folder_id, "cancel replay"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Created(info) => info,
            other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
        };
        let engine = crate::work_task::engine::test_engine(db);

        let accepted = cancel_linked_work_task(
            &engine,
            &EventEmitter::Noop,
            &conn,
            "platform-cancel-replay",
            "cancel-command-1",
            Some("用户停止平台任务"),
        )
        .await
        .unwrap();
        assert!(matches!(accepted, LinkedWorkTaskCancelOutcome::Accepted(_)));
        assert_eq!(
            work_task_service::get_model(&conn, task.id)
                .await
                .unwrap()
                .status,
            WorkTaskStatus::Canceled
        );
        assert!(
            work_task_service::requeue_canceled(&conn, task.id, None, &[], false)
                .await
                .unwrap()
        );
        let event_count_before_replay = work_task_event::Entity::find().count(&conn).await.unwrap();

        let replayed = cancel_linked_work_task(
            &engine,
            &EventEmitter::Noop,
            &conn,
            "platform-cancel-replay",
            "cancel-command-1",
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            replayed,
            LinkedWorkTaskCancelOutcome::Duplicate {
                local_work_task_id
            } if local_work_task_id == task.id
        ));
        assert_eq!(
            work_task_service::get_model(&conn, task.id)
                .await
                .unwrap()
                .status,
            WorkTaskStatus::Todo,
            "旧命令补发不能再次取消本地 requeue"
        );
        assert_eq!(
            work_task_event::Entity::find().count(&conn).await.unwrap(),
            event_count_before_replay,
            "duplicate receipt 不写第二条 canceled event"
        );
        let snapshot = reconcile_linked_work_task(&conn, "platform-cancel-replay")
            .await
            .unwrap();
        assert_eq!(snapshot.state, LinkedWorkTaskState::Cancelled);
        assert!(!snapshot.remote_workbench_available);

        let conflict = cancel_linked_work_task(
            &engine,
            &EventEmitter::Noop,
            &conn,
            "platform-cancel-replay",
            "cancel-command-2",
            None,
        )
        .await
        .expect_err("同一平台 Task 不能改写已有 receipt");
        assert!(conflict.to_string().contains("another cancel command"));
        assert_eq!(
            work_task_service::get_model(&conn, task.id)
                .await
                .unwrap()
                .status,
            WorkTaskStatus::Todo
        );
    }

    #[tokio::test]
    async fn cancel_of_an_already_canceled_task_only_commits_the_receipt() {
        let db = fresh_in_memory_db().await;
        let conn = db.conn.clone();
        let folder_id = seed_folder(&db, "/tmp/cerebro-cancel-existing").await;
        let task = match create_linked_work_task(
            &EventEmitter::Noop,
            &conn,
            "platform-already-canceled",
            draft(folder_id, "already canceled"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Created(info) => info,
            other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
        };
        assert!(work_task_service::cancel(&conn, task.id, None)
            .await
            .unwrap());
        let event_count = work_task_event::Entity::find().count(&conn).await.unwrap();
        let engine = crate::work_task::engine::test_engine(db);

        let outcome = cancel_linked_work_task(
            &engine,
            &EventEmitter::Noop,
            &conn,
            "platform-already-canceled",
            "cancel-command-existing",
            None,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, LinkedWorkTaskCancelOutcome::Accepted(_)));
        assert_eq!(
            work_task_event::Entity::find().count(&conn).await.unwrap(),
            event_count
        );
        let link = cerebro_task_link::Entity::find_by_id("platform-already-canceled")
            .one(&conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            link.cancel_command_id.as_deref(),
            Some("cancel-command-existing")
        );
    }

    async fn move_to_review(
        db: &crate::db::AppDatabase,
        task_id: i32,
        folder_id: i32,
        summary: &str,
    ) {
        let conversation_id = seed_conversation(db, folder_id, AgentType::Codex).await;
        let run_seq =
            work_task_service::claim_for_run(&db.conn, task_id, WorkTaskStatus::Todo, "test")
                .await
                .unwrap()
                .unwrap();
        assert!(work_task_service::begin_setup(&db.conn, task_id, run_seq)
            .await
            .unwrap());
        assert!(work_task_service::mark_running(
            &db.conn,
            task_id,
            run_seq,
            conversation_id,
            &format!("connection-{task_id}"),
        )
        .await
        .unwrap());
        assert!(work_task_service::settle_review(
            &db.conn,
            task_id,
            run_seq,
            Some(summary.to_string()),
            None,
        )
        .await
        .unwrap());
    }

    #[tokio::test]
    async fn rejected_cancel_rolls_back_its_tentative_receipt() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-cancel-rejected").await;
        let task = match create_linked_work_task(
            &EventEmitter::Noop,
            &db.conn,
            "platform-completed",
            draft(folder_id, "completed"),
        )
        .await
        .unwrap()
        {
            LinkedWorkTaskCreateOutcome::Created(info) => info,
            other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
        };
        move_to_review(&db, task.id, folder_id, "completed result").await;
        assert!(work_task_service::complete_without_merge(
            &db.conn,
            task.id,
            "accepted without merge",
        )
        .await
        .unwrap());
        let event_count = work_task_event::Entity::find()
            .count(&db.conn)
            .await
            .unwrap();

        let error = commit_linked_cancel(&db.conn, "platform-completed", "cancel-completed", None)
            .await
            .expect_err("done WorkTask 不能取消");
        assert!(error.to_string().contains("current state"));
        let link = cerebro_task_link::Entity::find_by_id("platform-completed")
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.cancel_command_id, None);
        assert_eq!(
            work_task_event::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            event_count
        );
    }

    #[tokio::test]
    async fn reconcile_reads_soft_deleted_todo_review_and_done_rows() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/cerebro-delete-reconcile").await;
        let mut task_ids = Vec::new();
        for (platform_id, title) in [
            ("deleted-todo", "todo"),
            ("deleted-review", "review"),
            ("deleted-done", "done"),
        ] {
            let task = match create_linked_work_task(
                &EventEmitter::Noop,
                &db.conn,
                platform_id,
                draft(folder_id, title),
            )
            .await
            .unwrap()
            {
                LinkedWorkTaskCreateOutcome::Created(info) => info,
                other => panic!("首次调用应创建 WorkTask，实际为 {other:?}"),
            };
            task_ids.push(task.id);
        }

        move_to_review(&db, task_ids[1], folder_id, "review result").await;
        move_to_review(&db, task_ids[2], folder_id, "done result").await;
        let mut review_row = work_task::Entity::find_by_id(task_ids[1])
            .one(&db.conn)
            .await
            .unwrap()
            .unwrap()
            .into_active_model();
        review_row.last_error = Set(Some("merge conflict on feature branch".to_string()));
        review_row.update(&db.conn).await.unwrap();
        assert_eq!(
            reconcile_linked_work_task(&db.conn, "deleted-review")
                .await
                .unwrap()
                .remote_workbench_available,
            true
        );
        assert!(work_task_service::complete_without_merge(
            &db.conn,
            task_ids[2],
            "accepted without merge",
        )
        .await
        .unwrap());

        for (task_id, status) in [
            (task_ids[0], WorkTaskStatus::Todo),
            (task_ids[1], WorkTaskStatus::Review),
            (task_ids[2], WorkTaskStatus::Done),
        ] {
            assert!(work_task_service::soft_delete(&db.conn, task_id, status)
                .await
                .unwrap());
        }

        let todo = reconcile_linked_work_task(&db.conn, "deleted-todo")
            .await
            .unwrap();
        let review = reconcile_linked_work_task(&db.conn, "deleted-review")
            .await
            .unwrap();
        let done = reconcile_linked_work_task(&db.conn, "deleted-done")
            .await
            .unwrap();
        assert_eq!(todo.state, LinkedWorkTaskState::Cancelled);
        assert_eq!(review.state, LinkedWorkTaskState::Cancelled);
        assert_eq!(done.state, LinkedWorkTaskState::Completed);
        assert!(todo.deleted && review.deleted && done.deleted);
        assert!(!todo.remote_workbench_available);
        assert!(!review.remote_workbench_available);
        assert!(!done.remote_workbench_available);
        assert_eq!(review.result_summary.as_deref(), Some("review result"));
        assert_eq!(done.result_summary.as_deref(), Some("done result"));
        assert_eq!(
            review.last_error.as_deref(),
            Some("merge conflict on feature branch")
        );
        assert_eq!(
            cerebro_task_link::Entity::find()
                .count(&db.conn)
                .await
                .unwrap(),
            3
        );
    }
}
