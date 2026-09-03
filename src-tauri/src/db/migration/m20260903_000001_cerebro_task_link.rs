use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CerebroTaskLink::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CerebroTaskLink::PlatformTaskId)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CerebroTaskLink::LocalWorkTaskId)
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(CerebroTaskLink::CancelCommandId)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(CerebroTaskLink::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // link 必须随 soft-deleted WorkTask 永久保留；这里不设置级联删除。
                    .foreign_key(
                        ForeignKey::create()
                            .from(CerebroTaskLink::Table, CerebroTaskLink::LocalWorkTaskId)
                            .to(WorkTask::Table, WorkTask::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(CerebroTaskLink::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CerebroTaskLink {
    Table,
    PlatformTaskId,
    LocalWorkTaskId,
    CancelCommandId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum WorkTask {
    Table,
    Id,
}
