use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::project::{BranchEntry, ProjectConfig};

#[derive(Serialize)]
pub struct ProjectListItem {
    #[serde(flatten)]
    pub config: ProjectConfig,
    pub branch_count: usize,
}

#[derive(Serialize)]
pub struct ProjectDetailResponse {
    #[serde(flatten)]
    pub config: ProjectConfig,
    pub branches: Vec<BranchEntry>,
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub allowed_operations: Vec<String>,
}

pub async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProjectListItem>>, StatusCode> {
    let projects = state
        .project_store
        .list_projects()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut out = Vec::with_capacity(projects.len());
    for p in projects {
        let branches = state
            .project_store
            .list_branches(p.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        out.push(ProjectListItem {
            branch_count: branches.len(),
            config: p,
        });
    }
    Ok(Json(out))
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectDetailResponse>, StatusCode> {
    let config = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let branches = state
        .project_store
        .list_branches(config.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ProjectDetailResponse { config, branches }))
}

pub async fn list_branches(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<BranchEntry>>, StatusCode> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let branches = state
        .project_store
        .list_branches(project.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(branches))
}

pub async fn update_config(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<ProjectConfig>, StatusCode> {
    let project = state
        .project_store
        .get_project_by_id(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let updated = state
        .project_store
        .update_allowed_ops(project.id, req.allowed_operations)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(updated))
}
