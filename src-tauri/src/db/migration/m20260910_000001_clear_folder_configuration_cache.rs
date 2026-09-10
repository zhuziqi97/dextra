use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 旧目录授权已清除，展示缓存必须重新从服务端取得。
        manager.get_connection().execute_unprepared("DELETE FROM app_metadata WHERE instr(key, 'cerebro.folder_configuration:') = 1").await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::service::app_metadata_service;

    #[tokio::test]
    async fn clears_only_folder_configuration_cache() {
        // 删除旧配置缓存，保留设备配对和其它本地设置。
        let db = crate::db::test_helpers::fresh_in_memory_db().await;
        app_metadata_service::upsert_value(&db.conn, "cerebro.folder_configuration:target", "old-config").await.unwrap();
        app_metadata_service::upsert_value(&db.conn, "cerebro.identity", "paired-device").await.unwrap();
        Migration.up(&SchemaManager::new(&db.conn)).await.unwrap();
        assert_eq!(app_metadata_service::get_value(&db.conn, "cerebro.folder_configuration:target").await.unwrap(), None);
        assert_eq!(app_metadata_service::get_value(&db.conn, "cerebro.identity").await.unwrap().as_deref(), Some("paired-device"));
    }
}
