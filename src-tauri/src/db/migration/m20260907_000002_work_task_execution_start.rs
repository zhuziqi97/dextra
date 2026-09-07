use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 历史运行缺少可定位边界，保持空值，恢复时不得猜测旧输出。
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .add_column(ColumnDef::new(WorkTask::ExecutionStart).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(WorkTask::Table)
                    .drop_column(WorkTask::ExecutionStart)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum WorkTask {
    Table,
    ExecutionStart,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        service::work_task_service,
        test_helpers::{fresh_in_memory_db, seed_folder},
    };
    use crate::models::{WorkTaskDraft, WorkTaskStatus};

    #[tokio::test]
    async fn execution_start_upgrade_preserves_old_task_without_inventing_boundary() {
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/execution-start-upgrade").await;
        let task = work_task_service::create(
            &db.conn,
            WorkTaskDraft {
                folder_id,
                title: "升级前任务".into(),
                config: serde_json::json!({"prompt_blocks":[{"type":"text","text":"检查代码"}]}),
            },
        )
        .await
        .unwrap();
        let seq = work_task_service::claim_for_run(&db.conn, task.id, WorkTaskStatus::Todo, "user")
            .await
            .unwrap()
            .unwrap();
        let before = work_task_service::get_model(&db.conn, task.id)
            .await
            .unwrap();
        let schema = SchemaManager::new(&db.conn);
        Migration.down(&schema).await.unwrap();
        assert!(!schema
            .has_column("work_task", "execution_start")
            .await
            .unwrap());
        Migration.up(&schema).await.unwrap();
        let upgraded = work_task_service::get_model(&db.conn, task.id)
            .await
            .unwrap();
        assert_eq!(upgraded.title, before.title);
        assert_eq!(upgraded.config, before.config);
        assert_eq!(upgraded.status, before.status);
        assert_eq!(upgraded.run_seq, seq);
        assert_eq!(upgraded.execution_start, None);
        assert_eq!(
            work_task_service::boot_reconcile_interrupted(&db.conn)
                .await
                .unwrap(),
            1
        );
        let recovered = work_task_service::get_model(&db.conn, task.id)
            .await
            .unwrap();
        assert_eq!(recovered.status, WorkTaskStatus::Failed);
        assert_eq!(recovered.result_summary, None);
        assert!(recovered
            .last_error
            .unwrap()
            .contains("缺少可定位的执行起点"));
    }
}
