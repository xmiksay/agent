use sea_orm::entity::prelude::*;
use serde::Serialize;

/// An operator-orderable backlog bucket. `priority` orders queue-vs-queue (higher
/// = sooner); the per-task in-queue ordering is `tasks.priority`. The scheduler
/// pulls the next `pending` task from across all queues whenever a slot frees.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "queues")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    pub priority: i16,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::tasks::Entity")]
    Task,
}

impl Related<super::tasks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Task.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
