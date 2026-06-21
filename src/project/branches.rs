use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::project_branches;
use crate::project::store::ProjectStore;

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

impl ProjectStore {
    pub async fn upsert_branch(
        &self,
        project_id: Uuid,
        new: NewBranchEntry,
    ) -> Result<BranchEntry> {
        let now: DateTime<Utc> = Utc::now();
        let existing = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::BranchSlug.eq(&new.branch_slug))
            .one(self.db())
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
            active.update(self.db()).await?;
            let model = project_branches::Entity::find_by_id(id)
                .one(self.db())
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
            .exec(self.db())
            .await
            .context("failed to insert branch")?;
        let model = project_branches::Entity::find_by_id(id)
            .one(self.db())
            .await?
            .ok_or_else(|| anyhow!("branch disappeared"))?;
        BranchEntry::from_model(model)
    }

    pub async fn list_branches(&self, project_id: Uuid) -> Result<Vec<BranchEntry>> {
        let rows = project_branches::Entity::find()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .order_by_asc(project_branches::Column::BranchSlug)
            .all(self.db())
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
            .one(self.db())
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
            .one(self.db())
            .await?;
        row.map(BranchEntry::from_model).transpose()
    }

    pub async fn delete_branch(&self, project_id: Uuid, branch_slug: &str) -> Result<()> {
        project_branches::Entity::delete_many()
            .filter(project_branches::Column::ProjectId.eq(project_id))
            .filter(project_branches::Column::BranchSlug.eq(branch_slug))
            .exec(self.db())
            .await
            .context("failed to delete branch")?;
        Ok(())
    }

    pub async fn touch_branch(&self, id: Uuid) -> Result<()> {
        let mut active: project_branches::ActiveModel = project_branches::Entity::find_by_id(id)
            .one(self.db())
            .await?
            .ok_or_else(|| anyhow!("branch not found"))?
            .into();
        active.last_used_at = Set(Utc::now().into());
        active.update(self.db()).await?;
        Ok(())
    }
}
