use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub status: String,
    pub trigger_type: String,
    pub trigger_data: Json,
    pub project_path: String,
    pub git_url: String,
    pub default_branch: String,
    pub created_at: DateTimeWithTimeZone,
    pub started_at: Option<DateTimeWithTimeZone>,
    pub finished_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::task_results::Entity")]
    TaskResult,
}

impl Related<super::task_results::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TaskResult.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
