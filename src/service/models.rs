//! A service's per-**trigger type** model mapping (`service_models`): which
//! catalog model the service runs for each kind of task. Split out of
//! `service/store.rs` (already over the 400-line cap).

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use sea_orm::*;
use uuid::Uuid;

use crate::entity::service_models;
use crate::jobs::types::TRIGGER_TYPES;
use crate::service::ServiceStore;

impl ServiceStore {
    /// The service's per-trigger-type model mapping (`trigger_type → model_id`).
    pub async fn trigger_models(&self, service_id: Uuid) -> Result<BTreeMap<String, Uuid>> {
        let rows = service_models::Entity::find()
            .filter(service_models::Column::ServiceId.eq(service_id))
            .all(self.db())
            .await
            .context("loading service model mapping")?;
        Ok(rows
            .into_iter()
            .map(|r| (r.trigger_type, r.model_id))
            .collect())
    }

    /// Replace the service's whole per-trigger-type mapping with `models`. Each
    /// key must be a known trigger type. Delete-then-insert: simple and correct at
    /// single-operator scale (no concurrent writers to the same service).
    pub async fn set_trigger_models(
        &self,
        service_id: Uuid,
        models: &BTreeMap<String, Uuid>,
    ) -> Result<()> {
        for trigger_type in models.keys() {
            if !TRIGGER_TYPES.contains(&trigger_type.as_str()) {
                bail!("unknown trigger_type '{trigger_type}'");
            }
        }
        service_models::Entity::delete_many()
            .filter(service_models::Column::ServiceId.eq(service_id))
            .exec(self.db())
            .await
            .context("clearing service model mapping")?;
        for (trigger_type, model_id) in models {
            service_models::Entity::insert(service_models::ActiveModel {
                id: Set(Uuid::new_v4()),
                service_id: Set(service_id),
                trigger_type: Set(trigger_type.clone()),
                model_id: Set(*model_id),
            })
            .exec(self.db())
            .await
            .context("inserting service model mapping")?;
        }
        Ok(())
    }
}
