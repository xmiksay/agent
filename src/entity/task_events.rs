use sea_orm::entity::prelude::*;
use serde::Serialize;

/// One persisted agent event. `(task_id, seq)` is the composite primary key and
/// the ordered read index; `seq` matches the live-stream envelope `seq`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "task_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub task_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub seq: i64,
    pub payload: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
