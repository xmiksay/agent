use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::entity::auth_requests;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthStatus {
    Pending,
    Approved,
    Denied,
}

impl AuthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthStatus::Pending => "pending",
            AuthStatus::Approved => "approved",
            AuthStatus::Denied => "denied",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "pending" => AuthStatus::Pending,
            "approved" => AuthStatus::Approved,
            "denied" => AuthStatus::Denied,
            other => return Err(anyhow!("unknown auth status: {other}")),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthRequest {
    pub id: Uuid,
    pub task_id: Uuid,
    pub requested_op: String,
    pub prompt_to_operator: String,
    pub status: AuthStatus,
    pub operator_reply: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

impl AuthRequest {
    pub fn from_model(m: auth_requests::Model) -> Result<Self> {
        Ok(Self {
            id: m.id,
            task_id: m.task_id,
            requested_op: m.requested_op,
            prompt_to_operator: m.prompt_to_operator,
            status: AuthStatus::parse(&m.status)?,
            operator_reply: m.operator_reply,
            created_at: m.created_at.into(),
            resolved_at: m.resolved_at.map(Into::into),
            metadata: m.metadata,
        })
    }
}

#[derive(Clone)]
pub struct AuthStore {
    db: DatabaseConnection,
}

impl AuthStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn create_pending(
        &self,
        task_id: Uuid,
        requested_op: String,
        prompt: String,
        metadata: Option<JsonValue>,
    ) -> Result<AuthRequest> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let active = auth_requests::ActiveModel {
            id: Set(id),
            task_id: Set(task_id),
            requested_op: Set(requested_op),
            prompt_to_operator: Set(prompt),
            status: Set(AuthStatus::Pending.as_str().to_string()),
            operator_reply: Set(None),
            created_at: Set(now.into()),
            resolved_at: Set(None),
            metadata: Set(metadata),
        };
        auth_requests::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("insert auth_request")?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("auth request disappeared"))
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<AuthRequest>> {
        let row = auth_requests::Entity::find_by_id(id).one(&self.db).await?;
        row.map(AuthRequest::from_model).transpose()
    }

    pub async fn list(&self, status: Option<AuthStatus>) -> Result<Vec<AuthRequest>> {
        self.list_filtered(status, None).await
    }

    pub async fn list_filtered(
        &self,
        status: Option<AuthStatus>,
        task_id: Option<Uuid>,
    ) -> Result<Vec<AuthRequest>> {
        let mut q = auth_requests::Entity::find()
            .order_by_desc(auth_requests::Column::CreatedAt);
        if let Some(s) = status {
            q = q.filter(auth_requests::Column::Status.eq(s.as_str()));
        }
        if let Some(t) = task_id {
            q = q.filter(auth_requests::Column::TaskId.eq(t));
        }
        let rows = q.all(&self.db).await?;
        rows.into_iter().map(AuthRequest::from_model).collect()
    }

    pub async fn resolve(
        &self,
        id: Uuid,
        decision: AuthStatus,
        reply: Option<String>,
    ) -> Result<AuthRequest> {
        if matches!(decision, AuthStatus::Pending) {
            return Err(anyhow!("cannot resolve to pending"));
        }
        let mut active: auth_requests::ActiveModel = auth_requests::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("auth request not found"))?
            .into();
        active.status = Set(decision.as_str().to_string());
        active.operator_reply = Set(reply);
        active.resolved_at = Set(Some(Utc::now().into()));
        active.update(&self.db).await?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("auth request disappeared"))
    }
}
