use sea_orm::entity::prelude::*;

/// Cerebro 平台 Task 与本地原生 WorkTask 的永久一对一关联。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "cerebro_task_link")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub platform_task_id: String,
    #[sea_orm(unique)]
    pub local_work_task_id: i32,
    #[sea_orm(column_type = "Text")]
    pub cancel_command_id: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::work_task::Entity",
        from = "Column::LocalWorkTaskId",
        to = "super::work_task::Column::Id"
    )]
    WorkTask,
}

impl Related<super::work_task::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::WorkTask.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
