use sea_orm::entity::prelude::*;
use serde::Serialize;

/// An agent backend that can run models. `kind` is the system-defined key the
/// code maps to a CLI (`claude_code` today — see `agent::backend_for`); `api_key`
/// is optional and only populated when the provider should run in API mode rather
/// than on a subscription login (injected into the agent's environment at spawn).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "model_providers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub kind: String,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    /// Never serialized to clients (write-only, like service tokens).
    #[serde(skip_serializing)]
    #[sea_orm(column_type = "Text", nullable)]
    pub api_key: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
