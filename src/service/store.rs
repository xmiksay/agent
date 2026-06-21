use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::Deserialize;
use uuid::Uuid;

use crate::entity::service;
use crate::project::ProviderKind;
use crate::service::types::{NewService, Service, UpdateService, build_credentials};

/// Distinguish an absent field (`None`) from an explicit `null` (`Some(None)`)
/// for `Option<Option<T>>` patch fields. Without this, serde collapses both to
/// `None` and a clear-to-null is indistinguishable from "leave unchanged".
pub(crate) fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

#[derive(Clone)]
pub struct ServiceStore {
    db: DatabaseConnection,
}

impl ServiceStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<Service>> {
        let rows = service::Entity::find()
            .order_by_asc(service::Column::Kind)
            .order_by_asc(service::Column::Slug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(Service::from_model).collect()
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<Service>> {
        let row = service::Entity::find_by_id(id).one(&self.db).await?;
        row.map(Service::from_model).transpose()
    }

    pub async fn get_by_slug(&self, kind: ProviderKind, slug: &str) -> Result<Option<Service>> {
        let row = service::Entity::find()
            .filter(service::Column::Kind.eq(kind.as_str()))
            .filter(service::Column::Slug.eq(slug))
            .one(&self.db)
            .await?;
        row.map(Service::from_model).transpose()
    }

    pub async fn create(&self, new: NewService) -> Result<Service> {
        validate_slug(&new.slug)?;
        // Reject an `app` service whose app_credentials are missing/malformed.
        build_credentials(
            new.kind,
            new.auth_kind,
            &new.token,
            &new.base_url,
            new.app_credentials.as_ref(),
        )?;

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = service::ActiveModel {
            id: Set(id),
            kind: Set(new.kind.as_str().to_string()),
            slug: Set(new.slug),
            display_name: Set(new.display_name),
            base_url: Set(new.base_url.trim_end_matches('/').to_string()),
            token: Set(new.token),
            webhook_secret: Set(new.webhook_secret),
            bot_username: Set(new.bot_username),
            autofire: Set(new.autofire),
            auth_kind: Set(new.auth_kind.as_str().to_string()),
            app_credentials: Set(new.app_credentials),
            trigger_mode: Set(new.trigger_mode.as_str().to_string()),
            trigger_label: Set(new.trigger_label),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        service::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert service")?;

        if let Some(models) = new.models {
            self.set_trigger_models(id, &models).await?;
        }
        if let Some(triggers) = &new.triggers {
            self.set_trigger_configs(id, triggers).await?;
        }

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("service disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateService) -> Result<Service> {
        let current = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow!("service not found"))?;

        // Resolve the post-patch state (None = keep current) and validate that an
        // `app` service still has well-formed credentials before writing.
        let auth_kind = upd.auth_kind.unwrap_or(current.auth_kind);
        let token = upd.token.clone().unwrap_or_else(|| current.token.clone());
        let app_credentials = upd.app_credentials.clone().or(current.app_credentials);
        let base_url = upd
            .base_url
            .clone()
            .unwrap_or_else(|| current.base_url.clone());
        build_credentials(
            current.kind,
            auth_kind,
            &token,
            &base_url,
            app_credentials.as_ref(),
        )?;

        let mut active: service::ActiveModel = service::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("service not found"))?
            .into();

        if let Some(v) = upd.display_name {
            active.display_name = Set(v);
        }
        if let Some(v) = upd.base_url {
            active.base_url = Set(v.trim_end_matches('/').to_string());
        }
        if let Some(v) = upd.token {
            active.token = Set(v);
        }
        if let Some(v) = upd.webhook_secret {
            active.webhook_secret = Set(v);
        }
        if let Some(v) = upd.bot_username {
            active.bot_username = Set(v);
        }
        if let Some(v) = upd.autofire {
            active.autofire = Set(v);
        }
        if let Some(v) = upd.auth_kind {
            active.auth_kind = Set(v.as_str().to_string());
        }
        if let Some(v) = upd.app_credentials {
            active.app_credentials = Set(Some(v));
        }
        if let Some(v) = upd.trigger_mode {
            active.trigger_mode = Set(v.as_str().to_string());
        }
        if let Some(v) = upd.trigger_label {
            active.trigger_label = Set(v);
        }
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;

        if let Some(models) = upd.models {
            self.set_trigger_models(id, &models).await?;
        }
        if let Some(triggers) = &upd.triggers {
            self.set_trigger_configs(id, triggers).await?;
        }

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("service disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = service::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete service")?;
        if res.rows_affected == 0 {
            bail!("service not found");
        }
        Ok(())
    }

    pub(crate) fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        bail!("slug is required");
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("slug must be ASCII alphanumeric, '-' or '_'");
    }
    Ok(())
}
