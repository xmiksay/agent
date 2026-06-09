//! CRUD over the `model_providers` table: the agent backends that run models.
//! `kind` is the system-defined key the code maps to a CLI (validated against
//! `agent::backend_for`); `api_key` is the optional API-mode secret, write-only
//! across the API like service tokens.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::model_providers;

/// A model provider as the app sees it. `api_key` is held for the runner but
/// never serialized — the API exposes only `has_api_key` (see `ProviderView`).
#[derive(Clone, Debug, Serialize)]
pub struct ModelProvider {
    pub id: Uuid,
    /// System-defined backend key (`claude_code`). Resolves the CLI.
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ModelProvider {
    fn from_model(m: model_providers::Model) -> Self {
        Self {
            id: m.id,
            kind: m.kind,
            name: m.name,
            api_key: m.api_key,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewModelProvider {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateModelProvider {
    pub kind: Option<String>,
    pub name: Option<String>,
    /// Outer `None` = leave unchanged; `Some(None)` = clear; `Some(Some(v))` = set.
    #[serde(default, deserialize_with = "crate::service::store::double_option")]
    pub api_key: Option<Option<String>>,
}

#[derive(Clone)]
pub struct ModelProviderStore {
    db: DatabaseConnection,
}

impl ModelProviderStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<ModelProvider>> {
        let rows = model_providers::Entity::find()
            .order_by_asc(model_providers::Column::Name)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(ModelProvider::from_model).collect())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<ModelProvider>> {
        let row = model_providers::Entity::find_by_id(id)
            .one(&self.db)
            .await?;
        Ok(row.map(ModelProvider::from_model))
    }

    pub async fn create(&self, new: NewModelProvider) -> Result<ModelProvider> {
        validate_kind(&new.kind)?;
        if new.name.trim().is_empty() {
            bail!("name is required");
        }
        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = model_providers::ActiveModel {
            id: Set(id),
            kind: Set(new.kind.trim().to_string()),
            name: Set(new.name.trim().to_string()),
            api_key: Set(normalize(new.api_key)),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        model_providers::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert model provider")?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("provider disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateModelProvider) -> Result<ModelProvider> {
        let mut active: model_providers::ActiveModel = model_providers::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("provider not found"))?
            .into();

        if let Some(v) = upd.kind {
            validate_kind(&v)?;
            active.kind = Set(v.trim().to_string());
        }
        if let Some(v) = upd.name {
            if v.trim().is_empty() {
                bail!("name must not be empty");
            }
            active.name = Set(v.trim().to_string());
        }
        if let Some(v) = upd.api_key {
            active.api_key = Set(normalize(v));
        }
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("provider disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        // The models FK is `RESTRICT`, so a provider that still has models can't
        // be deleted — surface that as a clean message rather than a raw DB error.
        let res = model_providers::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .map_err(|_| anyhow!("cannot delete a provider that still has models"))?;
        if res.rows_affected == 0 {
            bail!("provider not found");
        }
        Ok(())
    }
}

/// Reject a `kind` that has no agent backend wired (so a provider can't name a
/// CLI the runner can't launch).
fn validate_kind(kind: &str) -> Result<()> {
    crate::agent::backend_for(kind.trim()).map(|_| ())
}

fn normalize(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_kind_accepts_claude_code_rejects_unknown() {
        assert!(validate_kind("claude_code").is_ok());
        assert!(validate_kind("gpt").is_err());
    }

    #[test]
    fn new_provider_parses_optional_api_key() {
        let with: NewModelProvider =
            serde_json::from_str(r#"{ "kind": "claude_code", "name": "CC", "api_key": "sk" }"#)
                .unwrap();
        assert_eq!(with.api_key.as_deref(), Some("sk"));
        let without: NewModelProvider =
            serde_json::from_str(r#"{ "kind": "claude_code", "name": "CC" }"#).unwrap();
        assert!(without.api_key.is_none());
    }

    #[test]
    fn update_provider_api_key_distinguishes_clear_from_absent() {
        let absent: UpdateModelProvider = serde_json::from_str("{}").unwrap();
        assert!(absent.api_key.is_none());
        let cleared: UpdateModelProvider = serde_json::from_str(r#"{ "api_key": null }"#).unwrap();
        assert_eq!(cleared.api_key, Some(None));
        let set: UpdateModelProvider = serde_json::from_str(r#"{ "api_key": "sk" }"#).unwrap();
        assert_eq!(set.api_key, Some(Some("sk".into())));
    }
}
