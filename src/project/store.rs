use std::fmt;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::{project_branches, projects};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gitlab,
    Github,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Gitlab => "gitlab",
            ProviderKind::Github => "github",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "gitlab" => Ok(ProviderKind::Gitlab),
            "github" => Ok(ProviderKind::Github),
            other => Err(anyhow!("unknown provider: {other}")),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BranchStatus {
    Active,
    Idle,
    Releasing,
}

impl BranchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BranchStatus::Active => "active",
            BranchStatus::Idle => "idle",
            BranchStatus::Releasing => "releasing",
        }
    }
}

impl FromStr for BranchStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(BranchStatus::Active),
            "idle" => Ok(BranchStatus::Idle),
            "releasing" => Ok(BranchStatus::Releasing),
            other => Err(anyhow!("unknown branch status: {other}")),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectConfig {
    pub id: Uuid,
    pub provider: ProviderKind,
    pub service_id: Option<Uuid>,
    pub project_slug: String,
    pub full_name: String,
    pub remote_url: String,
    pub default_branch: String,
    pub my_username: String,
    pub allowed_operations: Vec<String>,
    /// `.env`-style minijinja template (`KEY=value` per line). Rendered with the
    /// task's runtime variables and injected into the agent process at spawn —
    /// see `project::env`.
    pub env_file: String,
    pub notes: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectConfig {
    fn from_model(m: projects::Model) -> Result<Self> {
        let allowed = serde_json::from_value::<Vec<String>>(m.allowed_operations)
            .context("invalid allowed_operations JSON")?;
        Ok(Self {
            id: m.id,
            provider: m.provider.parse()?,
            service_id: m.service_id,
            project_slug: m.project_slug,
            full_name: m.full_name,
            remote_url: m.remote_url,
            default_branch: m.default_branch,
            my_username: m.my_username,
            allowed_operations: allowed,
            env_file: m.env_file,
            notes: m.notes,
            created_at: m.created_at.into(),
            updated_at: m.updated_at.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewProjectConfig {
    pub provider: ProviderKind,
    pub service_id: Uuid,
    pub project_slug: String,
    pub full_name: String,
    pub remote_url: String,
    pub default_branch: String,
    pub my_username: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BranchEntry {
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_name: String,
    pub branch_slug: String,
    pub issue_iid: Option<i64>,
    pub pr_iid: Option<i64>,
    pub status: BranchStatus,
    pub checked_out_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
}

impl BranchEntry {
    fn from_model(m: project_branches::Model) -> Result<Self> {
        Ok(Self {
            id: m.id,
            project_id: m.project_id,
            branch_name: m.branch_name,
            branch_slug: m.branch_slug,
            issue_iid: m.issue_iid,
            pr_iid: m.pr_iid,
            status: m.status.parse()?,
            checked_out_at: m.checked_out_at.into(),
            last_used_at: m.last_used_at.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct NewBranchEntry {
    pub branch_name: String,
    pub branch_slug: String,
    pub issue_iid: Option<i64>,
    pub pr_iid: Option<i64>,
    pub status: BranchStatus,
}

pub fn default_allowed_operations() -> Vec<String> {
    vec![
        // Git — read
        "git status".into(),
        "git status *".into(),
        "git diff*".into(),
        "git log*".into(),
        "git show*".into(),
        "git branch*".into(),
        "git fetch*".into(),
        "git remote*".into(),
        "git ls-files*".into(),
        "git rev-parse*".into(),
        "git config --get*".into(),
        // Git — write (local only)
        "git add *".into(),
        "git commit *".into(),
        "git checkout *".into(),
        "git switch *".into(),
        "git stash*".into(),
        "git tag*".into(),
        "git restore *".into(),
        // Git — push (deny patterns in settings.json block --force / --mirror etc.)
        "git push".into(),
        "git push origin*".into(),
        // GitLab CLI
        "glab mr view*".into(),
        "glab mr list*".into(),
        "glab mr diff*".into(),
        "glab mr create*".into(),
        "glab mr note*".into(),
        "glab mr approve*".into(),
        "glab issue view*".into(),
        "glab issue list*".into(),
        "glab issue note*".into(),
        // GitHub CLI
        "gh pr view*".into(),
        "gh pr list*".into(),
        "gh pr diff*".into(),
        "gh pr create*".into(),
        "gh pr comment*".into(),
        "gh pr checks*".into(),
        "gh issue view*".into(),
        "gh issue list*".into(),
        "gh issue comment*".into(),
        // Cargo
        "cargo build*".into(),
        "cargo check*".into(),
        "cargo test*".into(),
        "cargo fmt*".into(),
        "cargo clippy*".into(),
        "cargo run*".into(),
        "cargo doc*".into(),
        "cargo tree*".into(),
        "cargo metadata*".into(),
        // Node
        "npm run *".into(),
        "npm test*".into(),
        "npm ci".into(),
        "npx *".into(),
        "make*".into(),
        // Read-only file ops (bare form for pipe consumers, * form for invocations with args)
        "cat".into(),
        "cat *".into(),
        "head".into(),
        "head *".into(),
        "tail".into(),
        "tail *".into(),
        "wc".into(),
        "wc *".into(),
        "grep *".into(),
        "rg *".into(),
        "find *".into(),
        "ls".into(),
        "ls *".into(),
        "pwd".into(),
        "which *".into(),
        "test *".into(),
        "file *".into(),
        "stat *".into(),
        // Text processing (pipe targets)
        "jq".into(),
        "jq *".into(),
        "sort".into(),
        "sort *".into(),
        "uniq".into(),
        "uniq *".into(),
        "awk *".into(),
        "sed *".into(),
        "cut *".into(),
        "tr *".into(),
        "column".into(),
        "column *".into(),
        "xargs grep *".into(),
        "xargs cat *".into(),
        // File create-only
        "mkdir -p *".into(),
    ]
}

#[derive(Clone)]
pub struct ProjectStore {
    db: DatabaseConnection,
}

impl ProjectStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Upsert a project row. The returned bool is `true` only when a new row was
    /// inserted (the caller uses it to auto-register the webhook once).
    pub async fn upsert_project(&self, new: NewProjectConfig) -> Result<(ProjectConfig, bool)> {
        if let Some(existing) = self.get_project(new.service_id, &new.project_slug).await? {
            let mut active: projects::ActiveModel = projects::Entity::find_by_id(existing.id)
                .one(&self.db)
                .await?
                .ok_or_else(|| anyhow!("project disappeared"))?
                .into();

            let mut changed = false;
            if existing.full_name != new.full_name {
                active.full_name = Set(new.full_name.clone());
                changed = true;
            }
            if existing.remote_url != new.remote_url {
                active.remote_url = Set(new.remote_url.clone());
                changed = true;
            }
            if existing.default_branch != new.default_branch {
                active.default_branch = Set(new.default_branch.clone());
                changed = true;
            }
            if existing.my_username != new.my_username {
                active.my_username = Set(new.my_username.clone());
                changed = true;
            }
            if existing.service_id != Some(new.service_id) {
                active.service_id = Set(Some(new.service_id));
                changed = true;
            }
            if changed {
                active.updated_at = Set(Utc::now().into());
                active.update(&self.db).await?;
                let updated = self
                    .get_project(new.service_id, &new.project_slug)
                    .await?
                    .ok_or_else(|| anyhow!("project disappeared after update"))?;
                return Ok((updated, false));
            }
            return Ok((existing, false));
        }

        let now: DateTime<Utc> = Utc::now();
        let id = Uuid::new_v4();
        let active = projects::ActiveModel {
            id: Set(id),
            provider: Set(new.provider.as_str().to_string()),
            service_id: Set(Some(new.service_id)),
            project_slug: Set(new.project_slug.clone()),
            full_name: Set(new.full_name),
            remote_url: Set(new.remote_url),
            default_branch: Set(new.default_branch),
            my_username: Set(new.my_username),
            allowed_operations: Set(serde_json::to_value(default_allowed_operations())?),
            env_file: Set(String::new()),
            notes: Set(String::new()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };
        projects::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert project")?;

        let created = self
            .get_project(new.service_id, &new.project_slug)
            .await?
            .ok_or_else(|| anyhow!("project disappeared after insert"))?;
        Ok((created, true))
    }

    pub async fn get_project(&self, service_id: Uuid, slug: &str) -> Result<Option<ProjectConfig>> {
        let row = projects::Entity::find()
            .filter(projects::Column::ServiceId.eq(service_id))
            .filter(projects::Column::ProjectSlug.eq(slug))
            .one(&self.db)
            .await?;
        row.map(ProjectConfig::from_model).transpose()
    }

    pub async fn get_project_by_id(&self, id: Uuid) -> Result<Option<ProjectConfig>> {
        let row = projects::Entity::find_by_id(id).one(&self.db).await?;
        row.map(ProjectConfig::from_model).transpose()
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectConfig>> {
        let rows = projects::Entity::find()
            .order_by_asc(projects::Column::Provider)
            .order_by_asc(projects::Column::ProjectSlug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(ProjectConfig::from_model).collect()
    }

    pub async fn update_allowed_ops(&self, id: Uuid, ops: Vec<String>) -> Result<ProjectConfig> {
        let mut active: projects::ActiveModel = projects::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("project not found"))?
            .into();
        active.allowed_operations = Set(serde_json::to_value(ops)?);
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;
        self.get_project_by_id(id)
            .await?
            .ok_or_else(|| anyhow!("project disappeared"))
    }

    pub async fn update_env_file(&self, id: Uuid, env_file: String) -> Result<ProjectConfig> {
        let mut active: projects::ActiveModel = projects::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("project not found"))?
            .into();
        active.env_file = Set(env_file);
        active.updated_at = Set(Utc::now().into());
        active.update(&self.db).await?;
        self.get_project_by_id(id)
            .await?
            .ok_or_else(|| anyhow!("project disappeared"))
    }

    pub async fn upsert_branch(
        &self,
        project_id: Uuid,
        new: NewBranchEntry,
    ) -> Result<BranchEntry> {
        let now: DateTime<Utc> = Utc::now();
        let existing = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::BranchSlug.eq(&new.branch_slug))
            .one(&self.db)
            .await?;

        if let Some(model) = existing {
            let id = model.id;
            let mut active: project_branches::ActiveModel = model.into();
            active.branch_name = Set(new.branch_name);
            if new.issue_iid.is_some() {
                active.issue_iid = Set(new.issue_iid);
            }
            if new.pr_iid.is_some() {
                active.pr_iid = Set(new.pr_iid);
            }
            active.status = Set(new.status.as_str().to_string());
            active.last_used_at = Set(now.into());
            active.update(&self.db).await?;
            let model = project_branches::Entity::find_by_id(id)
                .one(&self.db)
                .await?
                .ok_or_else(|| anyhow!("branch disappeared"))?;
            return BranchEntry::from_model(model);
        }

        let id = Uuid::new_v4();
        let active = project_branches::ActiveModel {
            id: Set(id),
            project_id: Set(project_id),
            branch_name: Set(new.branch_name),
            branch_slug: Set(new.branch_slug),
            issue_iid: Set(new.issue_iid),
            pr_iid: Set(new.pr_iid),
            status: Set(new.status.as_str().to_string()),
            checked_out_at: Set(now.into()),
            last_used_at: Set(now.into()),
        };
        project_branches::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert branch")?;
        let model = project_branches::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("branch disappeared"))?;
        BranchEntry::from_model(model)
    }

    pub async fn list_branches(&self, project_id: Uuid) -> Result<Vec<BranchEntry>> {
        let rows = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .order_by_asc(project_branches::Column::BranchSlug)
            .all(&self.db)
            .await?;
        rows.into_iter().map(BranchEntry::from_model).collect()
    }

    pub async fn find_branch(
        &self,
        project_id: Uuid,
        branch_slug: &str,
    ) -> Result<Option<BranchEntry>> {
        let row = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::BranchSlug.eq(branch_slug))
            .one(&self.db)
            .await?;
        row.map(BranchEntry::from_model).transpose()
    }

    pub async fn find_branch_for_issue(
        &self,
        project_id: Uuid,
        issue_iid: i64,
    ) -> Result<Option<BranchEntry>> {
        let row = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::IssueIid.eq(issue_iid))
            .one(&self.db)
            .await?;
        row.map(BranchEntry::from_model).transpose()
    }

    pub async fn delete_branch(&self, project_id: Uuid, branch_slug: &str) -> Result<()> {
        project_branches::Entity::delete_many()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::BranchSlug.eq(branch_slug))
            .exec(&self.db)
            .await
            .context("failed to delete branch")?;
        Ok(())
    }

    pub async fn touch_branch(&self, id: Uuid) -> Result<()> {
        let mut active: project_branches::ActiveModel = project_branches::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow!("branch not found"))?
            .into();
        active.last_used_at = Set(Utc::now().into());
        active.update(&self.db).await?;
        Ok(())
    }
}
