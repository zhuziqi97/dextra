use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 重新取得包含执行项目元数据的配置；不改配对、绑定或授权。
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
    async fn execution_project_cache_refresh_preserves_pairing() {
        // 旧展示缓存不再用于回显，设备身份和本地目录保留。
        let db = crate::db::test_helpers::fresh_in_memory_db().await;
        let folder = crate::db::test_helpers::seed_folder(&db, "/tmp/project-cache-test").await;
        app_metadata_service::upsert_value(&db.conn, "cerebro.folder_configuration:target", "old-display-cache").await.unwrap();
        app_metadata_service::upsert_value(&db.conn, "cerebro.identity", "paired-device").await.unwrap();
        Migration.up(&SchemaManager::new(&db.conn)).await.unwrap();
        assert!(app_metadata_service::get_value(&db.conn, "cerebro.folder_configuration:target").await.unwrap().is_none());
        assert_eq!(app_metadata_service::get_value(&db.conn, "cerebro.identity").await.unwrap().as_deref(), Some("paired-device"));
        use sea_orm::EntityTrait;
        assert!(crate::db::entities::folder::Entity::find_by_id(folder).one(&db.conn).await.unwrap().is_some());
    }
}
