mod agent;
mod api;
mod auth;
mod config;
mod entity;
mod git_service;
mod jobs;
mod project;
mod provider;
mod spa;
mod webhook;
mod workspace;
mod ws;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::middleware as axum_middleware;
use axum::routing::{get, post, put};
use axum::Router;
use anyhow::Context;
use migration::MigratorTrait;
use sea_orm::Database;
use tracing::info;

use crate::auth::store::AuthStore;
use crate::auth::waiter::AuthWaiter;
use crate::config::Config;
use crate::git_service::GitServiceStore;
use crate::jobs::hub::LiveSessions;
use crate::jobs::output_log::TaskOutputLog;
use crate::jobs::registry::RunningTasks;
use crate::jobs::store::TaskStore;
use crate::project::ProjectStore;
use crate::provider::ProviderRegistry;
use crate::workspace::Workspace;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub task_store: Arc<TaskStore>,
    pub project_store: Arc<ProjectStore>,
    pub git_service_store: GitServiceStore,
    pub workspace: Arc<Workspace>,
    pub providers: ProviderRegistry,
    pub auth_store: Arc<AuthStore>,
    pub auth_waiter: AuthWaiter,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env()?;

    let db = Database::connect(&config.database_url).await?;
    migration::Migrator::up(&db, None).await?;

    let git_service_store = GitServiceStore::new(db.clone());
    let providers = ProviderRegistry::new(git_service_store.clone());
    providers.reload().await?;

    let project_store = Arc::new(ProjectStore::new(db.clone()));
    let workspace = Arc::new(Workspace::new(&config.repo_base_path));
    workspace
        .install_shared_hooks()
        .await
        .context("installing shared agent hooks")?;
    let auth_store = Arc::new(AuthStore::new(db.clone()));
    let auth_waiter = AuthWaiter::new();
    let output_log = TaskOutputLog::new();
    let running = RunningTasks::new();
    let hub = LiveSessions::new(db.clone());
    let task_store = Arc::new(TaskStore::new(
        db,
        config.clone(),
        providers.clone(),
        project_store.clone(),
        workspace.clone(),
        output_log,
        running,
        hub,
    ));

    // Any task left running/pending in the DB was orphaned by a previous
    // process — flip those rows to `killed` so the UI matches reality.
    match task_store.recover_orphans().await {
        Ok(0) => {}
        Ok(n) => tracing::info!(recovered = n, "marked orphan tasks as killed"),
        Err(e) => tracing::warn!(error = %e, "failed to recover orphan tasks"),
    }

    let state = AppState {
        config: config.clone(),
        task_store,
        project_store,
        git_service_store,
        workspace,
        providers,
        auth_store,
        auth_waiter,
    };

    let api_routes = Router::new()
        .route(
            "/api/tasks",
            get(api::handlers::list_tasks).post(api::handlers::create_task),
        )
        .route("/api/tasks/stats", get(api::stats::task_stats))
        .route(
            "/api/tasks/{id}",
            get(api::handlers::get_task)
                .patch(api::handlers::edit_task)
                .delete(api::handlers::delete_task),
        )
        .route("/api/tasks/{id}/confirm", post(api::handlers::confirm_task))
        .route("/api/tasks/{id}/retry", post(api::handlers::retry_task))
        .route("/api/tasks/{id}/kill", post(api::handlers::kill_task))
        .route("/api/tasks/{id}/continue", post(api::handlers::continue_task))
        .route("/api/tasks/{id}/message", post(api::handlers::push_message))
        .route("/api/tasks/{id}/diff", get(api::handlers::task_diff))
        .route("/api/tasks/{id}/events", get(api::handlers::task_events))
        .route("/api/projects", get(api::projects::list_projects))
        .route(
            "/api/projects/{id}",
            get(api::projects::get_project),
        )
        .route(
            "/api/projects/{id}/config",
            put(api::projects::update_config),
        )
        .route(
            "/api/projects/{id}/branches",
            get(api::projects::list_branches),
        )
        .route(
            "/api/git_services",
            get(api::git_services::list).post(api::git_services::create),
        )
        .route(
            "/api/git_services/{id}",
            get(api::git_services::get)
                .put(api::git_services::update)
                .delete(api::git_services::delete),
        )
        .route(
            "/api/auth_requests",
            get(api::auth_requests::list_auth_requests),
        )
        .route(
            "/api/auth_requests/{id}",
            get(api::auth_requests::get_auth_request),
        )
        .route(
            "/api/auth_requests/{id}/resolve",
            post(api::auth_requests::resolve_auth_request),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_bearer,
        ))
        // Cheap probe the SPA uses to detect whether the token is valid.
        .route(
            "/api/auth/check",
            get(StatusCode::NO_CONTENT).route_layer(axum_middleware::from_fn_with_state(
                state.clone(),
                auth::middleware::require_bearer,
            )),
        );

    let app = Router::new()
        .route("/webhook/gitlab/{slug}", post(webhook::gitlab::handle))
        .route("/webhook/github/{slug}", post(webhook::github::handle))
        .route("/internal/authcheck", post(auth::handlers::authcheck))
        // The live stream authorizes via a `?token=` query param inside the
        // handler (browsers can't set headers on a WebSocket), so it sits
        // outside the bearer middleware that guards `/api/*`.
        .route("/ws/tasks/{id}", get(ws::task_stream))
        .merge(api_routes)
        .route("/health", get(health))
        .with_state(state)
        .fallback(spa::handler);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    info!(addr = %config.listen_addr, "server starting");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn health() -> StatusCode {
    StatusCode::OK
}
