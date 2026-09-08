use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // HIST-COMPAT[CLIENT-HISTORY]：用户明确保留旧任务关联与会话正文；旧 cerebro_task_link
        // 仅保留历史表，当前业务不读写。用户确认历史导出或清理后才删除。
        // 模块授权已归属于服务端目录配置，会话正文与原生任务内容保持原样。
        manager.alter_table(Table::alter().table(Conversation::Table).drop_column(Conversation::CerebroSelection).to_owned()).await?;
        manager.get_connection().execute_unprepared("UPDATE work_task SET config = json_remove(config, '$.cerebro_selection') WHERE json_type(config, '$.cerebro_selection') IS NOT NULL").await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter().table(Conversation::Table).add_column(ColumnDef::new(Conversation::CerebroSelection).text().null()).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Conversation { Table, CerebroSelection }

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{EntityTrait, ConnectionTrait};

    #[tokio::test]
    async fn replacing_selection_preserves_conversation() {
        use crate::db::test_helpers::{fresh_in_memory_db, seed_conversation, seed_folder};
        let db = fresh_in_memory_db().await;
        let folder_id = seed_folder(&db, "/tmp/client-configuration-migration").await;
        let id = seed_conversation(&db, folder_id, crate::models::AgentType::Codex).await;
        let schema = SchemaManager::new(&db.conn);
        Migration.down(&schema).await.unwrap();
        db.conn.execute_unprepared("UPDATE conversation SET cerebro_selection = '{\"mode\":\"LOCAL\"}'").await.unwrap();
        Migration.up(&schema).await.unwrap();
        let row = crate::db::entities::conversation::Entity::find_by_id(id).one(&db.conn).await.unwrap().unwrap();
        assert_eq!(row.folder_id, folder_id);
    }
}
