use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::middleware as axum_middleware;
use axum::routing::{get, post, put};
use migration::MigratorTrait;
use sea_orm::Database;
use tracing::info;

use agent::AppState;
use agent::auth;
use agent::auth::store::AuthStore;
use agent::auth::waiter::AuthWaiter;
use agent::config::Config;
use agent::jobs::hub::LiveSessions;
use agent::jobs::registry::RunningTasks;
use agent::jobs::store::TaskStore;
use agent::models::{ModelStore, ProviderStore};
use agent::project::ProjectStore;
use agent::provider::ProviderRegistry;
use agent::service::ServiceStore;
use agent::workspace::Workspace;
use agent::{api, spa, webhook, ws};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env()?;
    config.log_summary();

    let db = Database::connect(&config.database_url).await?;
    migration::Migrator::up(&db, None).await?;

    let service_store = ServiceStore::new(db.clone());
    let model_store = ModelStore::new(db.clone());
    let provider_store = ProviderStore::new(db.clone());
    let providers = ProviderRegistry::new(service_store.clone());
    providers.reload().await?;

    let project_store = Arc::new(ProjectStore::new(db.clone()));
    let workspace = Arc::new(Workspace::new(&config.repo_base_path));
    let auth_store = Arc::new(AuthStore::new(db.clone()));
    let auth_waiter = AuthWaiter::new();
    let running = RunningTasks::new();
    let hub = LiveSessions::new(db.clone());
    let task_store = Arc::new(TaskStore::new(
        db,
        config.clone(),
        providers.clone(),
        project_store.clone(),
        workspace.clone(),
        running,
        hub,
        auth_store.clone(),
        auth_waiter.clone(),
    ));

    // A task left mid-flight (task_state=working_on, no finished_at) was
    // orphaned by a previous process — reconcile those rows to failed so the UI
    // matches reality.
    match task_store.recover_orphans().await {
        Ok(0) => {}
        Ok(n) => tracing::info!(recovered = n, "reconciled orphan tasks as failed"),
        Err(e) => tracing::warn!(error = %e, "failed to recover orphan tasks"),
    }

    let state = AppState {
        config: config.clone(),
        task_store,
        project_store,
        service_store,
        model_store,
        provider_store,
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
        .route("/api/tasks/bulk", post(api::bulk::bulk_action))
        .route(
            "/api/tasks/{id}",
            get(api::handlers::get_task)
                .patch(api::handlers::edit_task)
                .delete(api::handlers::delete_task),
        )
        .route("/api/tasks/{id}/confirm", post(api::handlers::confirm_task))
        .route("/api/tasks/{id}/retry", post(api::handlers::retry_task))
        .route("/api/tasks/{id}/kill", post(api::handlers::kill_task))
        .route(
            "/api/tasks/{id}/continue",
            post(api::handlers::continue_task),
        )
        .route("/api/tasks/{id}/message", post(api::handlers::push_message))
        .route("/api/tasks/{id}/diff", get(api::handlers::task_diff))
        .route("/api/tasks/{id}/events", get(api::handlers::task_events))
        .route(
            "/api/projects",
            get(api::projects::list_projects).post(api::projects::create_project),
        )
        .route("/api/projects/{id}", get(api::projects::get_project))
        .route(
            "/api/projects/{id}/config",
            put(api::projects::update_config),
        )
        .route("/api/projects/{id}/env", put(api::projects::update_env))
        .route(
            "/api/projects/{id}/branches",
            get(api::projects::list_branches),
        )
        .route(
            "/api/projects/{id}/register_webhook",
            post(api::projects::register_webhook),
        )
        .route(
            "/api/providers",
            get(api::providers::list).post(api::providers::create),
        )
        .route(
            "/api/providers/{id}",
            get(api::providers::get)
                .put(api::providers::update)
                .delete(api::providers::delete),
        )
        .route(
            "/api/models",
            get(api::models::list).post(api::models::create),
        )
        .route(
            "/api/models/{id}",
            get(api::models::get)
                .put(api::models::update)
                .delete(api::models::delete),
        )
        .route(
            "/api/services",
            get(api::services::list).post(api::services::create),
        )
        .route(
            "/api/services/{id}",
            get(api::services::get)
                .put(api::services::update)
                .delete(api::services::delete),
        )
        .route(
            "/api/services/{id}/github_app/install",
            get(api::services::github_app_install),
        )
        .route(
            "/api/services/{id}/github_app/sync",
            post(api::services::github_app_sync),
        )
        .route(
            "/api/services/{id}/gitlab_token/provision",
            post(api::gitlab_token::provision),
        )
        .route(
            "/api/services/{id}/gitlab_token/rotate",
            post(api::gitlab_token::rotate),
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
        // GitHub redirects the operator's browser here after an App install; it
        // carries no bearer, so it sits outside the `/api/*` middleware (trust is
        // the `state` param naming an existing service).
        .route(
            "/github_app/callback",
            get(api::services::github_app_callback),
        )
        // Single app-wide live stream. Auth is in-band (the client's first frame
        // is its token), so it sits outside the `/api/*` bearer middleware.
        .route("/ws", get(ws::global_stream))
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
