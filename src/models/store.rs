//! The model catalog: CRUD over the `models` table plus run-time resolution
//! (`resolve`, `resolve_default`) that joins a model to its provider so the
//! runner gets the backend `kind` + optional API key alongside the `model_id`.
//! A model pairs a `provider_id` (which backend/CLI runs it) with the `model_id`
//! that CLI is given, a human `alias`, a per-1M-token price table, and optional
//! `thinking`/`effort` settings.

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::prelude::Expr;
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{model_providers, models};

/// A catalog model as the app sees it. No secrets, so it doubles as the API view.
#[derive(Clone, Debug, Serialize)]
pub struct AiModel {
    pub id: Uuid,
    /// The owning `model_providers` row (which backend runs this model).
    pub provider_id: Uuid,
    /// The id handed to the provider CLI (`--model`), e.g. `claude-opus-4-8`.
    pub model_id: String,
    /// Human-facing name shown in the UI, e.g. `Opus 4.8`.
    pub alias: String,
    /// USD per 1M tokens.
    pub input_price: f64,
    pub output_price: f64,
    pub cache_write_price: f64,
    pub cache_read_price: f64,
    /// Enable extended thinking for this model.
    pub thinking: bool,
    /// Reasoning effort (`low` | `medium` | `high`), if the provider honors it.
    pub effort: Option<String>,
    /// The single global fallback model (used when neither task nor service picks one).
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AiModel {
    fn from_model(m: models::Model) -> Self {
        Self {
            id: m.id,
            provider_id: m.provider_id,
            model_id: m.model_id,
            alias: m.alias,
            input_price: m.input_price,
            output_price: m.output_price,
            cache_write_price: m.cache_write_price,
            cache_read_price: m.cache_read_price,
            thinking: m.thinking,
            effort: m.effort,
            is_default: m.is_default,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        }
    }
}

/// A model joined to its provider, as the runner needs it: the CLI `model_id`,
/// the backend `provider_kind` (resolves which CLI), and the provider's optional
/// `api_key` (set → run in API mode; injected into the agent's environment).
#[derive(Clone, Debug)]
pub struct ResolvedModel {
    pub model_id: String,
    pub alias: String,
    pub provider_kind: String,
    pub api_key: Option<String>,
}

impl ResolvedModel {
    fn join(m: models::Model, p: model_providers::Model) -> Self {
        Self {
            model_id: m.model_id,
            alias: m.alias,
            provider_kind: p.kind,
            api_key: p.api_key,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewModel {
    pub provider_id: Uuid,
    pub model_id: String,
    pub alias: String,
    #[serde(default)]
    pub input_price: f64,
    #[serde(default)]
    pub output_price: f64,
    #[serde(default)]
    pub cache_write_price: f64,
    #[serde(default)]
    pub cache_read_price: f64,
    #[serde(default)]
    pub thinking: bool,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateModel {
    pub provider_id: Option<Uuid>,
    pub model_id: Option<String>,
    pub alias: Option<String>,
    pub input_price: Option<f64>,
    pub output_price: Option<f64>,
    pub cache_write_price: Option<f64>,
    pub cache_read_price: Option<f64>,
    pub thinking: Option<bool>,
    /// Outer `None` = leave unchanged; `Some(None)` = clear; `Some(Some(v))` = set.
    #[serde(default, deserialize_with = "crate::service::store::double_option")]
    pub effort: Option<Option<String>>,
    pub is_default: Option<bool>,
}

#[derive(Clone)]
pub struct ModelStore {
    db: DatabaseConnection,
}

impl ModelStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<AiModel>> {
        let rows = models::Entity::find()
            .order_by_asc(models::Column::Alias)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(AiModel::from_model).collect())
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<AiModel>> {
        let row = models::Entity::find_by_id(id).one(&self.db).await?;
        Ok(row.map(AiModel::from_model))
    }

    /// Resolve a model id to the runner's view (joins its provider).
    pub async fn resolve(&self, id: Uuid) -> Result<Option<ResolvedModel>> {
        let Some(m) = models::Entity::find_by_id(id).one(&self.db).await? else {
            return Ok(None);
        };
        let p = model_providers::Entity::find_by_id(m.provider_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("model provider {} missing", m.provider_id))?;
        Ok(Some(ResolvedModel::join(m, p)))
    }

    /// Resolve the global default model to the runner's view, if one is flagged.
    pub async fn resolve_default(&self) -> Result<Option<ResolvedModel>> {
        let Some(m) = models::Entity::find()
            .filter(models::Column::IsDefault.eq(true))
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let p = model_providers::Entity::find_by_id(m.provider_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("model provider {} missing", m.provider_id))?;
        Ok(Some(ResolvedModel::join(m, p)))
    }

    /// The global default model's id, if one is flagged.
    pub async fn default_model_id(&self) -> Result<Option<Uuid>> {
        Ok(models::Entity::find()
            .filter(models::Column::IsDefault.eq(true))
            .one(&self.db)
            .await?
            .map(|m| m.id))
    }

    pub async fn create(&self, new: NewModel) -> Result<AiModel> {
        validate(&new.model_id, &new.alias)?;
        // Only one row may be the global default — clear any prior holder first.
        if new.is_default {
            self.clear_default().await?;
        }

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = models::ActiveModel {
            id: Set(id),
            provider_id: Set(new.provider_id),
            model_id: Set(new.model_id.trim().to_string()),
            alias: Set(new.alias.trim().to_string()),
            input_price: Set(new.input_price),
            output_price: Set(new.output_price),
            cache_write_price: Set(new.cache_write_price),
            cache_read_price: Set(new.cache_read_price),
            thinking: Set(new.thinking),
            effort: Set(normalize_effort(new.effort)),
            is_default: Set(new.is_default),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        models::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert model")?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("model disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateModel) -> Result<AiModel> {
        let mut active: models::ActiveModel = models::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("model not found"))?
            .into();

        // Promoting this row to default demotes any other default first.
        if upd.is_default == Some(true) {
            self.clear_default().await?;
        }

        if let Some(v) = upd.provider_id {
            active.provider_id = Set(v);
        }
        if let Some(v) = upd.model_id {
            if v.trim().is_empty() {
                bail!("model_id must not be empty");
            }
            active.model_id = Set(v.trim().to_string());
        }
        if let Some(v) = upd.alias {
            if v.trim().is_empty() {
                bail!("alias must not be empty");
            }
            active.alias = Set(v.trim().to_string());
        }
        if let Some(v) = upd.input_price {
            active.input_price = Set(v);
        }
        if let Some(v) = upd.output_price {
            active.output_price = Set(v);
        }
        if let Some(v) = upd.cache_write_price {
            active.cache_write_price = Set(v);
        }
        if let Some(v) = upd.cache_read_price {
            active.cache_read_price = Set(v);
        }
        if let Some(v) = upd.thinking {
            active.thinking = Set(v);
        }
        if let Some(v) = upd.effort {
            active.effort = Set(normalize_effort(v));
        }
        if let Some(v) = upd.is_default {
            active.is_default = Set(v);
        }
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("model disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = models::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete model")?;
        if res.rows_affected == 0 {
            bail!("model not found");
        }
        Ok(())
    }

    /// Demote whichever row currently holds the global default (so a new default
    /// can be set without tripping the one-default unique index).
    async fn clear_default(&self) -> Result<()> {
        models::Entity::update_many()
            .col_expr(models::Column::IsDefault, Expr::value(false))
            .filter(models::Column::IsDefault.eq(true))
            .exec(&self.db)
            .await
            .context("clearing previous default model")?;
        Ok(())
    }
}

fn validate(model_id: &str, alias: &str) -> Result<()> {
    if model_id.trim().is_empty() {
        bail!("model_id is required");
    }
    if alias.trim().is_empty() {
        bail!("alias is required");
    }
    Ok(())
}

/// Treat blank effort as "unset" so the UI's empty field round-trips to `None`.
fn normalize_effort(effort: Option<String>) -> Option<String> {
    effort
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_model_defaults_prices_and_flags() {
        let new: NewModel = serde_json::from_str(
            r#"{ "provider_id": "00000000-0000-0000-0000-0000000c0de0",
                 "model_id": "claude-opus-4-8", "alias": "Opus" }"#,
        )
        .unwrap();
        assert_eq!(new.input_price, 0.0);
        assert!(!new.thinking);
        assert!(!new.is_default);
    }

    #[test]
    fn normalize_effort_blanks_to_none() {
        assert_eq!(normalize_effort(Some("  ".into())), None);
        assert_eq!(normalize_effort(Some("high".into())), Some("high".into()));
        assert_eq!(normalize_effort(None), None);
    }

    #[test]
    fn update_model_effort_distinguishes_clear_from_absent() {
        // Absent → no change; explicit null → clear; explicit value → set.
        let absent: UpdateModel = serde_json::from_str("{}").unwrap();
        assert!(absent.effort.is_none());
        let cleared: UpdateModel = serde_json::from_str(r#"{ "effort": null }"#).unwrap();
        assert_eq!(cleared.effort, Some(None));
        let set: UpdateModel = serde_json::from_str(r#"{ "effort": "low" }"#).unwrap();
        assert_eq!(set.effort, Some(Some("low".into())));
    }
}
