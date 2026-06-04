use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "auth_requests")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub requested_op: String,
    #[sea_orm(column_type = "Text")]
    pub prompt_to_operator: String,
    pub status: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub operator_reply: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub resolved_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tasks::Entity",
        from = "Column::TaskId",
        to = "super::tasks::Column::Id",
        on_delete = "Cascade"
    )]
    Task,
}

impl Related<super::tasks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Task.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
