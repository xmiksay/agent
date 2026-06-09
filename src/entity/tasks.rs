use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "tasks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Durable backing of the derived `agent_state` axis — narrowed to
    /// `cold | pending | failed`. The volatile `warm`/`running` distinction is
    /// NOT stored here; it's overlaid at read time from the live hub (see
    /// `derive_agent_state`). Never serialized to the client — `TaskView` emits
    /// the derived value under the `agent_state` key instead (`skip_serializing`
    /// here keeps the flattened entity from colliding with it).
    #[serde(skip_serializing)]
    pub agent_state: String,
    /// Operator-owned human lifecycle: `pending | working_on | completed |
    /// failed`. Auto-advanced by the runner, freely operator-overridable.
    pub task_state: String,
    pub trigger_type: String,
    pub trigger_data: Json,
    pub created_at: DateTimeWithTimeZone,
    pub started_at: Option<DateTimeWithTimeZone>,
    pub finished_at: Option<DateTimeWithTimeZone>,
    pub branch: Option<String>,
    /// The work item's project. Everything else about where/how the task runs —
    /// remote URL, default branch, provider, owning service — is resolved through
    /// this at run time (`project_id → projects → service`), not duplicated here.
    pub project_id: Uuid,
    /// The current/working claude CLI session id, for `--resume`. Set once the
    /// agent emits its init frame; the per-run history lives in `task_sessions`.
    #[sea_orm(column_type = "Text", nullable)]
    pub session_id: Option<String>,
    pub pid: Option<i64>,
    #[sea_orm(column_type = "Text", nullable)]
    pub pending_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::task_sessions::Entity")]
    TaskSession,
    #[sea_orm(has_many = "super::auth_requests::Entity")]
    AuthRequest,
    #[sea_orm(
        belongs_to = "super::projects::Entity",
        from = "Column::ProjectId",
        to = "super::projects::Column::Id",
        on_delete = "Cascade"
    )]
    Project,
}

impl Related<super::task_sessions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TaskSession.def()
    }
}

impl Related<super::auth_requests::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AuthRequest.def()
    }
}

impl Related<super::projects::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
