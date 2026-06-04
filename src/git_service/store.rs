use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::git_services;
use crate::project::ProviderKind;

#[derive(Clone, Debug, Serialize)]
pub struct GitService {
    pub id: Uuid,
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    pub token: String,
    pub webhook_secret: String,
    pub bot_username: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GitService {
    fn from_model(m: git_services::Model) -> Result<Self> {
        Ok(Self {
            id: m.id,
            kind: m.kind.parse()?,
            slug: m.slug,
            display_name: m.display_name,
            base_url: m.base_url,
            token: m.token,
            webhook_secret: m.webhook_secret,
            bot_username: m.bot_username,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewGitService {
    pub kind: ProviderKind,
    pub slug: String,
    pub display_name: String,
    pub base_url: String,
    pub token: String,
    pub webhook_secret: String,
    pub bot_username: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateGitService {
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub webhook_secret: Option<String>,
    pub bot_username: Option<String>,
}

#[derive(Clone)]
pub struct GitServiceStore {
    db: DatabaseConnection,
}

impl GitServiceStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<GitService>> {
        let rows = git_services::Entity::find()
            .order_by_asc(git_services::Column::Kind)
            .order_by_asc(git_services::Column::Slug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(GitService::from_model).collect()
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<GitService>> {
        let row = git_services::Entity::find_by_id(id).one(&self.db).await?;
        row.map(GitService::from_model).transpose()
    }

    pub async fn get_by_slug(&self, kind: ProviderKind, slug: &str) -> Result<Option<GitService>> {
        let row = git_services::Entity::find()
            .filter(git_services::Column::Kind.eq(kind.as_str()))
            .filter(git_services::Column::Slug.eq(slug))
            .one(&self.db)
            .await?;
        row.map(GitService::from_model).transpose()
    }

    pub async fn create(&self, new: NewGitService) -> Result<GitService> {
        validate_slug(&new.slug)?;
        if matches!(new.kind, ProviderKind::Github) {
            let exists = git_services::Entity::find()
                .filter(git_services::Column::Kind.eq(new.kind.as_str()))
                .one(&self.db)
                .await?;
            if exists.is_some() {
                bail!("a github service is already configured");
            }
        }

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = git_services::ActiveModel {
            id: Set(id),
            kind: Set(new.kind.as_str().to_string()),
            slug: Set(new.slug),
            display_name: Set(new.display_name),
            base_url: Set(new.base_url.trim_end_matches('/').to_string()),
            token: Set(new.token),
            webhook_secret: Set(new.webhook_secret),
            bot_username: Set(new.bot_username),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        git_services::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert git_service")?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("git_service disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateGitService) -> Result<GitService> {
        let mut active: git_services::ActiveModel = git_services::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("git_service not found"))?
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
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;

        self.get(id)
            .await?
            .ok_or_else(|| anyhow!("git_service disappeared after update"))
    }

    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = git_services::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete git_service")?;
        if res.rows_affected == 0 {
            bail!("git_service not found");
        }
        Ok(())
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
