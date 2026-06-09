use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "projects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub provider: String,
    pub project_slug: String,
    pub full_name: String,
    pub remote_url: String,
    pub default_branch: String,
    pub my_username: String,
    pub allowed_operations: Json,
    #[sea_orm(column_type = "Text")]
    pub env_file: String,
    #[sea_orm(column_type = "Text")]
    pub notes: String,
    pub service_id: Option<Uuid>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project_branches::Entity")]
    Branch,
    #[sea_orm(
        belongs_to = "super::service::Entity",
        from = "Column::ServiceId",
        to = "super::service::Column::Id",
        on_delete = "SetNull"
    )]
    Service,
}

impl Related<super::project_branches::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Branch.def()
    }
}

impl Related<super::service::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Service.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
