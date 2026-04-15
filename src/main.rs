mod api;
mod config;
mod entity;
mod gitlab;
mod jobs;
mod views;
mod webhook;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use migration::MigratorTrait;
use minijinja::Environment;
use sea_orm::Database;
use tracing::info;

use crate::config::Config;
use crate::gitlab::client::GitLabClient;
use crate::jobs::store::TaskStore;
use crate::webhook::handler::handle_webhook;
use crate::webhook::verify::verify_token;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub task_store: Arc<TaskStore>,
    pub templates: Arc<Environment<'static>>,
}

fn init_templates() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    env
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gitlab_claude_agent=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env()?;

    let db = Database::connect(&config.database_url).await?;
    migration::Migrator::up(&db, None).await?;

    let gitlab = GitLabClient::new(&config.gitlab_url, &config.gitlab_token);
    let task_store = Arc::new(TaskStore::new(db, config.clone(), gitlab));
    let templates = Arc::new(init_templates());

    let state = AppState {
        config: config.clone(),
        task_store,
        templates,
    };

    let webhook_routes = Router::new()
        .route("/webhook/gitlab", post(handle_webhook))
        .route_layer(middleware::from_fn_with_state(state.clone(), verify_token));

    let api_routes = Router::new()
        .route("/api/tasks", get(api::handlers::list_tasks))
        .route("/api/tasks/{id}", get(api::handlers::get_task))
        .route("/api/tasks/{id}/confirm", post(api::handlers::confirm_task));

    let html_routes = Router::new()
        .route("/", get(views::tasks_page))
        .route("/tasks/{id}", get(views::task_detail_page))
        .route("/tasks/{id}/confirm", post(views::confirm_task_page));

    let app = Router::new()
        .merge(webhook_routes)
        .merge(api_routes)
        .merge(html_routes)
        .route("/health", get(health))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!(addr = %config.listen_addr, "server starting");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}
