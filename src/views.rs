use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, Redirect};
use minijinja::context;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

pub async fn tasks_page(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, StatusCode> {
    let tasks = state
        .task_store
        .list_tasks(query.status.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let html = state
        .templates
        .get_template("tasks.html")
        .and_then(|tmpl| {
            tmpl.render(context! {
                tasks => tasks,
                status => query.status,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Html(html))
}

pub async fn task_detail_page(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, StatusCode> {
    let (task, result) = state
        .task_store
        .get_task(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let trigger_json = serde_json::to_string_pretty(&task.trigger_data)
        .unwrap_or_default();

    let html = state
        .templates
        .get_template("task_detail.html")
        .and_then(|tmpl| {
            tmpl.render(context! {
                task => task,
                result => result,
                trigger_json => trigger_json,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Html(html))
}

pub async fn confirm_task_page(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Redirect, (StatusCode, String)> {
    state
        .task_store
        .confirm_task(id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Redirect::to(&format!("/tasks/{id}")))
}
