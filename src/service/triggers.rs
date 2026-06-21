//! Per-trigger-type gating config (`service_triggers`). A row OVERRIDES the
//! service-level `trigger_mode`/`trigger_label` default for one trigger type;
//! absence means "enabled with the service defaults". Split out of `store.rs`
//! (already over the 400-line cap), mirroring the sibling `models` module.

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::service_triggers;
use crate::jobs::types::TRIGGER_TYPES;
use crate::service::store::ServiceStore;
use crate::service::types::TriggerMode;

/// The gating config for one trigger type. `mode`/`label` are only meaningful
/// for the issue type; the other types gate on `enabled` alone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub enabled: bool,
    pub mode: TriggerMode,
    pub label: String,
}

impl ServiceStore {
    /// Load the per-type configs for a service, keyed by `trigger_type`. A type
    /// with no row is simply absent — callers treat absence as the default.
    pub async fn trigger_configs(
        &self,
        service_id: Uuid,
    ) -> Result<BTreeMap<String, TriggerConfig>> {
        let rows = service_triggers::Entity::find()
            .filter(service_triggers::Column::ServiceId.eq(service_id))
            .all(self.db())
            .await
            .context("loading service_triggers")?;
        let mut out = BTreeMap::new();
        for r in rows {
            let mode = TriggerMode::from_str(&r.mode)
                .with_context(|| format!("invalid stored trigger mode {:?}", r.mode))?;
            out.insert(
                r.trigger_type,
                TriggerConfig {
                    enabled: r.enabled,
                    mode,
                    label: r.label,
                },
            );
        }
        Ok(out)
    }

    /// Replace the service's whole per-type config with `configs`. Each key must
    /// be a known trigger type. Delete-then-insert: simple and correct at
    /// single-operator scale (no concurrent writers to the same service).
    pub async fn set_trigger_configs(
        &self,
        service_id: Uuid,
        configs: &BTreeMap<String, TriggerConfig>,
    ) -> Result<()> {
        validate_trigger_types(configs)?;

        service_triggers::Entity::delete_many()
            .filter(service_triggers::Column::ServiceId.eq(service_id))
            .exec(self.db())
            .await
            .context("clearing service_triggers")?;

        for (trigger_type, cfg) in configs {
            service_triggers::Entity::insert(service_triggers::ActiveModel {
                id: Set(Uuid::new_v4()),
                service_id: Set(service_id),
                trigger_type: Set(trigger_type.clone()),
                enabled: Set(cfg.enabled),
                mode: Set(cfg.mode.as_str().to_string()),
                label: Set(cfg.label.clone()),
            })
            .exec(self.db())
            .await
            .context("inserting service_trigger")?;
        }
        Ok(())
    }
}

/// Reject any key that isn't a known trigger type. Pure validation — no DB.
fn validate_trigger_types(configs: &BTreeMap<String, TriggerConfig>) -> Result<()> {
    for key in configs.keys() {
        if !TRIGGER_TYPES.contains(&key.as_str()) {
            bail!("unknown trigger_type '{key}'");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TriggerConfig {
        TriggerConfig {
            enabled: true,
            mode: TriggerMode::Assignee,
            label: String::new(),
        }
    }

    #[test]
    fn rejects_unknown_trigger_type() {
        let mut configs = BTreeMap::new();
        configs.insert("bogus".to_string(), cfg());
        let err = validate_trigger_types(&configs).unwrap_err().to_string();
        assert!(err.contains("unknown trigger_type"), "got: {err}");
    }

    #[test]
    fn accepts_known_trigger_types() {
        let mut configs = BTreeMap::new();
        for t in TRIGGER_TYPES {
            configs.insert(t.to_string(), cfg());
        }
        assert!(validate_trigger_types(&configs).is_ok());
    }
}
