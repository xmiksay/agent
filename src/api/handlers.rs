use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::jobs::lifecycle::derive_agent_state;
use crate::jobs::store::TaskEdits;
use crate::jobs::types::TriggerReason;
use crate::project::ProviderKind;
use crate::provider::resolve_token;
use crate::service::Service;

#[derive(Deserialize)]
pub struct ListQuery {
    /// SQL filter on the persisted operator lifecycle column.
    pub task_state: Option<String>,
    /// In-memory filter on the derived runtime disposition (see note in
    /// `list_tasks`).
    pub agent_state: Option<String>,
}

/// A task as the SPA sees it: the persisted entity flattened, but with the two
/// orthogonal state axes made explicit — `task_state` (persisted) and
/// `agent_state` (DERIVED at read time, overlaying the live hub onto the durable
/// backing). The entity's durable `agent_state` column is shadowed by the
/// derived value here. No `live` boolean — `agent_state ∈ {warm, running}`
/// carries that signal now.
#[derive(Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: crate::entity::tasks::Model,
    /// Derived runtime disposition: `cold|warm|pending|running|failed`. The
    /// entity's durable `agent_state` column is `skip_serializing`, so this is the
    /// only `agent_state` key in the output.
    pub agent_state: &'static str,
    /// Fields resolved from the task's project (no longer columns on `tasks`):
    /// the API keeps emitting them so the SPA can render provider/project/remote
    /// without a second fetch. `None` if the project was deleted.
    pub project_path: Option<String>,
    pub provider: Option<String>,
    pub service_id: Option<Uuid>,
    pub default_branch: Option<String>,
    pub git_url: Option<String>,
}

#[derive(Serialize)]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: TaskView,
    pub result: Option<crate::entity::task_sessions::Model>,
    /// Absolute path to this task's git worktree on the agent host.
    /// `None` if the task's service can no longer be resolved.
    pub work_dir: Option<String>,
}

/// Build the read-time task view: overlay the derived `agent_state` onto the
/// persisted entity and resolve the project-derived fields the SPA renders.
async fn task_view(task: crate::entity::tasks::Model, state: &AppState) -> TaskView {
    let agent_state = derive_agent_state(&task.agent_state, task.id, state.task_store.hub());
    let project = state
        .project_store
        .get_project_by_id(task.project_id)
        .await
        .ok()
        .flatten();
    let (project_path, provider, service_id, default_branch, git_url) = match project {
        Some(p) => (
            Some(p.full_name),
            Some(p.provider.as_str().to_string()),
            p.service_id,
            Some(p.default_branch),
            Some(p.remote_url),
        ),
        None => (None, None, None, None, None),
    };
    TaskView {
        task,
        agent_state,
        project_path,
        provider,
        service_id,
        default_branch,
        git_url,
    }
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<TaskView>>, StatusCode> {
    let tasks = state
        .task_store
        .list_tasks(query.task_state.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // task_state filters in SQL; agent_state is derived (not a column) so it's
    // filtered in-memory here. Acceptable at single-operator scale where the task
    // list is small.
    let want = query.agent_state.as_deref();
    let mut views = Vec::with_capacity(tasks.len());
    for t in tasks {
        let v = task_view(t, &state).await;
        if want.is_none_or(|w| v.agent_state == w) {
            views.push(v);
        }
    }

    Ok(Json(views))
}

/// Resolve a task's git worktree on disk via its project → service, returning
/// the owning `Service` (for credentials + kind) alongside the absolute path.
/// `None` if the project was deleted or carries no service. Shared by `get_task`
/// (path only) and `refresh_token` (path + credentials).
async fn resolve_task_worktree(
    state: &AppState,
    task: &crate::entity::tasks::Model,
) -> Option<(Service, std::path::PathBuf)> {
    use crate::workspace::layout::slugify;

    let project = state
        .project_store
        .get_project_by_id(task.project_id)
        .await
        .ok()
        .flatten()?;
    let svc = state
        .task_store
        .providers()
        .service(project.service_id?)
        .await?;
    let project_slug = slugify(&project.full_name);
    let branch = task
        .branch
        .clone()
        .unwrap_or_else(|| project.default_branch.clone());
    let branch_slug = slugify(&branch);
    let dir = state
        .task_store
        .workspace()
        .branch_dir(&svc.slug, &project_slug, &branch_slug);
    Some((svc, dir))
}

pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDetail>, StatusCode> {
    let (task, result) = state
        .task_store
        .get_task(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let work_dir = resolve_task_worktree(&state, &task)
        .await
        .map(|(_, dir)| dir.to_string_lossy().into_owned());

    let task = task_view(task, &state).await;
    Ok(Json(TaskDetail {
        task,
        result,
        work_dir,
    }))
}

#[derive(Deserialize, Default)]
pub struct RefreshTokenBody {
    /// Operator-supplied token to push directly — covers the case where minting
    /// is broken. Absent/blank → re-resolve from the service credentials.
    pub token: Option<String>,
}

/// Rotate the token the agent itself uses: rewrite this task's `agent.env`
/// (sourced per command via `BASH_ENV`) with a fresh token. The mid-turn escape
/// hatch for a single turn that outlives the App token's ~1h TTL — env can't be
/// auto-refreshed mid-turn (#52). Works whether the agent is live or idle: it
/// only rewrites a file the agent reads on its next command.
pub async fn refresh_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Option<Json<RefreshTokenBody>>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (task, _) = state
        .task_store
        .get_task(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "task not found".to_string()))?;

    let (svc, work_dir) = resolve_task_worktree(&state, &task).await.ok_or((
        StatusCode::NOT_FOUND,
        "task worktree unresolved".to_string(),
    ))?;

    let token_var = match svc.kind {
        ProviderKind::Github => "GH_TOKEN",
        ProviderKind::Gitlab => "GITLAB_TOKEN",
    };

    let supplied = body
        .and_then(|b| b.0.token)
        .filter(|t| !t.trim().is_empty());
    let token = match supplied {
        Some(t) => t,
        None => {
            let creds = svc
                .credentials()
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            resolve_token(&creds)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
    };

    crate::workspace::write_agent_env(&work_dir, token_var, &token)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn confirm_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .confirm_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
pub struct RetryResponse {
    pub task_id: Uuid,
}

#[derive(Deserialize)]
pub struct CreateTaskBody {
    pub project_id: Uuid,
    pub trigger: TriggerReason,
}

/// Operator-driven counterpart to the webhook dispatcher: pick a project,
/// hand over a fully-formed TriggerReason, get a pending task. Useful when
/// the webhook never arrived or was filtered out.
pub async fn create_task(
    State(state): State<AppState>,
    Json(payload): Json<CreateTaskBody>,
) -> Result<(StatusCode, Json<RetryResponse>), (StatusCode, String)> {
    let project = state
        .project_store
        .get_project_by_id(payload.project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "project not found".to_string()))?;

    let id = state
        .task_store
        .create_task(payload.trigger, project.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(RetryResponse { task_id: id })))
}

pub async fn retry_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryResponse>, (StatusCode, String)> {
    let new_id = state
        .task_store
        .retry_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(RetryResponse { task_id: new_id }))
}

pub async fn kill_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .kill_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

/// One persisted hub frame, as seen by the SPA. `kind` is `event`,
/// `auth_request`, or `status`; rows of different kinds are interleaved in `seq`
/// order. The explicit `seq` (not the array index) is authoritative for deduping
/// REST history against live WebSocket frames.
#[derive(Serialize)]
pub struct PersistedEvent {
    pub seq: i64,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Serialize)]
pub struct EventsResponse {
    /// Persisted hub frames for the task, ordered by `seq` ascending.
    pub events: Vec<PersistedEvent>,
}

/// Durable frame history from `events`. Live frames for an active task
/// arrive over the WebSocket; this seeds the timeline (and is the only source
/// once a task has finished and its in-memory session is gone).
pub async fn task_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<EventsResponse>, (StatusCode, String)> {
    let events = state
        .task_store
        .task_events(id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?
        .into_iter()
        .map(|r| PersistedEvent {
            seq: r.seq,
            kind: r.kind,
            payload: r.payload,
        })
        .collect();
    Ok(Json(EventsResponse { events }))
}

pub async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .delete_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn continue_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryResponse>, (StatusCode, String)> {
    let new_id = state
        .task_store
        .continue_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(RetryResponse { task_id: new_id }))
}

#[derive(Deserialize)]
pub struct PushMessageBody {
    pub body: String,
}

pub async fn push_message(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<PushMessageBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .push_message(id, payload.body)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn edit_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<TaskEdits>,
) -> Result<StatusCode, (StatusCode, String)> {
    state
        .task_store
        .update_task(id, payload)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Serialize)]
pub struct DiffResponse {
    pub diff: String,
}

pub async fn task_diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DiffResponse>, (StatusCode, String)> {
    let diff = state
        .task_store
        .branch_diff(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(DiffResponse { diff }))
}

#[cfg(test)]
mod tests {
    use super::TaskView;
    use chrono::Utc;
    use uuid::Uuid;

    /// The serialized TaskView must emit the DERIVED `agent_state` (not the
    /// entity's durable column value) and the persisted `task_state`, with no
    /// `live` key. Guards the frontend JSON contract: the entity's durable
    /// `agent_state` is `skip_serializing`, so the derived overlay is the only one.
    #[test]
    fn taskview_emits_derived_agent_state() {
        let id = Uuid::new_v4();
        let model = crate::entity::tasks::Model {
            id,
            // Durable backing says "failed"...
            agent_state: "failed".to_string(),
            task_state: "working_on".to_string(),
            trigger_type: "issue".to_string(),
            trigger_data: serde_json::json!({}),
            created_at: Utc::now().into(),
            started_at: None,
            finished_at: None,
            branch: Some("b".to_string()),
            project_id: Uuid::new_v4(),
            session_id: None,
            pid: None,
            pending_message: None,
            model_id: None,
        };
        // ...but the derived overlay says "running".
        let view = TaskView {
            task: model,
            agent_state: "running",
            project_path: Some("acme/widgets".to_string()),
            provider: Some("github".to_string()),
            service_id: None,
            default_branch: Some("main".to_string()),
            git_url: None,
        };
        let v = serde_json::to_value(&view).unwrap();

        assert_eq!(v["agent_state"], "running", "derived agent_state wins");
        assert_eq!(v["task_state"], "working_on", "task_state passes through");
        assert!(v.get("live").is_none(), "no live boolean");
        assert_eq!(v["id"], id.to_string());
    }
}
