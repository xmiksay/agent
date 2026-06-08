use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "git_services")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub kind: String,
    pub slug: String,
    pub display_name: String,
    #[sea_orm(column_type = "Text")]
    pub base_url: String,
    #[sea_orm(column_type = "Text")]
    pub token: String,
    #[sea_orm(column_type = "Text")]
    pub webhook_secret: String,
    pub bot_username: String,
    pub autofire: bool,
    /// `pat` (GitHub/GitLab PATs and GitLab Group/Project Access Tokens) or
    /// `app` (GitHub App #9; GitLab has no `app` flow) — the type half of the
    /// credential. `app_credentials` holds the value half: the provider-specific
    /// secret bundle, populated only when `auth_kind = 'app'`.
    pub auth_kind: String,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub app_credentials: Option<Json>,
    /// `assignee` | `label` | `both` — how an issue event triggers the agent.
    /// `trigger_label` names the label watched when the mode includes labels.
    pub trigger_mode: String,
    #[sea_orm(column_type = "Text")]
    pub trigger_label: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::projects::Entity")]
    Project,
}

impl Related<super::projects::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
