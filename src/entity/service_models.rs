use sea_orm::entity::prelude::*;
use serde::Serialize;

/// Per-service, per-**trigger type** model selection: which catalog model a
/// service runs for each kind of task (`issue`, `review_mr`, `fix_review`,
/// `mr_comment`, `issue_comment`). Unique on `(service_id, trigger_type)`. Both
/// FKs cascade-delete, so a removed service or model leaves no dangling mapping.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "service_models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub trigger_type: String,
    pub model_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
