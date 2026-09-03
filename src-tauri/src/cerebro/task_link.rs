use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait};

use crate::commands::work_task::nudge_pump;
use crate::db::entities::{cerebro_task_link, work_task};
use crate::db::error::DbError;
use crate::db::service::work_task_service;
use crate::models::{WorkTaskDraft, WorkTaskInfo};
use crate::web::event_bridge::{emit_event, EventEmitter, WorkTaskChange, WORK_TASK_CHANGED_EVENT};

#[derive(Debug)]
pub enum LinkedWorkTaskCreateOutcome {
    Created(WorkTaskInfo),
    Duplicate(WorkTaskInfo),
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sea_orm::{Database, EntityTrait, PaginatorTrait};
    use sea_orm_migration::MigratorTrait;

    use super::*;
    use crate::db::entities::{cerebro_task_link, work_task, work_task_event};
    use crate::db::migration::Migrator;
    use crate::db::service::work_task_service;
    use crate::db::test_helpers::{fresh_disk_db, fresh_in_memory_db, seed_folder};
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
}
