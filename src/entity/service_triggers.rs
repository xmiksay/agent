use sea_orm::entity::prelude::*;
use serde::Serialize;

/// Per-service, per-**trigger type** gating override (`service_triggers`): each
/// row overrides the service-level `trigger_mode`/`trigger_label` default for one
/// trigger type (`issue`, `review_mr`, `fix_review`, `mr_comment`,
/// `issue_comment`) and can disable it. Unique on `(service_id, trigger_type)`.
/// The FK cascade-deletes, so a removed service leaves no dangling rows.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "service_triggers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub service_id: Uuid,
    pub trigger_type: String,
    pub enabled: bool,
    pub mode: String,
    #[sea_orm(column_type = "Text")]
    pub label: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::service::Entity",
        from = "Column::ServiceId",
        to = "super::service::Column::Id",
        on_delete = "Cascade"
    )]
    Service,
}

impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
