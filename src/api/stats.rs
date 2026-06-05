//! Time-spent aggregation across tasks. Per task, "spent" is
//! `finished_at - started_at` for finished rows and `now - started_at` for
//! still-running rows. Tasks that never started (pending, killed-before-run)
//! contribute zero seconds but still count toward the row's task count.
//!
//! Rolled up per `group_by` (project / service / trigger_type) within the
//! `[from, to)` window applied to `created_at`.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::entity::tasks;

#[derive(Deserialize)]
pub struct StatsQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// `project` (default), `service`, `branch`, or `trigger_type`.
    pub group_by: Option<String>,
}

#[derive(Serialize)]
pub struct StatsRow {
    pub key: String,
    pub label: String,
    pub task_count: i64,
    pub total_secs: i64,
}

#[derive(Serialize)]
pub struct StatsResponse {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub group_by: String,
    pub total_tasks: i64,
    pub total_secs: i64,
    pub rows: Vec<StatsRow>,
}

pub async fn task_stats(
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<StatsResponse>, (StatusCode, String)> {
    let to = q.to.unwrap_or_else(Utc::now);
    let from = q.from.unwrap_or_else(|| to - chrono::Duration::days(30));
    let group_by = q.group_by.as_deref().unwrap_or("project");
    if !["project", "service", "branch", "trigger_type"].contains(&group_by) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "group_by must be project|service|branch|trigger_type, got {group_by}",
            ),
        ));
    }

    let rows = tasks::Entity::find()
        .filter(tasks::Column::CreatedAt.gte(from))
        .filter(tasks::Column::CreatedAt.lt(to))
        .all(state.task_store.db())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Service slugs for service-grouped labels.
    let services = state
        .git_service_store
        .list()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let service_slug: HashMap<Uuid, String> =
        services.into_iter().map(|s| (s.id, s.slug)).collect();

    let now = Utc::now();
    let mut buckets: HashMap<String, StatsRow> = HashMap::new();
    let mut total_secs: i64 = 0;
    let total_tasks = rows.len() as i64;

    for t in rows {
        let secs = duration_secs(&t, now);
        total_secs += secs;
        let (key, label) = match group_by {
            "project" => (
                t.project_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "_no_project".into()),
                t.project_path.clone(),
            ),
            "service" => {
                let key = t
                    .git_service_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "_no_service".into());
                let label = t
                    .git_service_id
                    .and_then(|id| service_slug.get(&id).cloned())
                    .unwrap_or_else(|| "(no service)".into());
                (key, label)
            }
            "branch" => {
                // Branch labels are unique enough on their own; but qualify the
                // key with project_path so the same branch name on two repos
                // doesn't collapse into one row.
                let branch = t.branch.clone().unwrap_or_else(|| "(no branch)".into());
                let key = format!("{}::{branch}", t.project_path);
                let label = format!("{branch}  · {}", t.project_path);
                (key, label)
            }
            _ => (t.trigger_type.clone(), t.trigger_type.clone()),
        };
        let entry = buckets.entry(key.clone()).or_insert(StatsRow {
            key,
            label,
            task_count: 0,
            total_secs: 0,
        });
        entry.task_count += 1;
        entry.total_secs += secs;
    }

    let mut rows: Vec<StatsRow> = buckets.into_values().collect();
    rows.sort_by(|a, b| b.total_secs.cmp(&a.total_secs).then(a.label.cmp(&b.label)));

    Ok(Json(StatsResponse {
        from,
        to,
        group_by: group_by.into(),
        total_tasks,
        total_secs,
        rows,
    }))
}

fn duration_secs(t: &tasks::Model, now: DateTime<Utc>) -> i64 {
    let started = match t.started_at {
        Some(s) => DateTime::<Utc>::from(s),
        None => return 0,
    };
    let end = match t.finished_at {
        Some(f) => DateTime::<Utc>::from(f),
        None if t.status == "running" => now,
        None => return 0,
    };
    (end - started).num_seconds().max(0)
}
