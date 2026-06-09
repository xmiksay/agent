//! Library crate for the agent service. The binary (`main.rs`) is a thin shell
//! that wires these modules into an axum app; exposing them here also lets the
//! `tests/` integration suite drive the store and DB-backed flows directly.

use std::sync::Arc;

pub mod agent;
pub mod api;
pub mod auth;
pub mod config;
pub mod entity;
pub mod jobs;
pub mod models;
pub mod project;
pub mod provider;
pub mod service;
pub mod spa;
pub mod webhook;
pub mod workspace;
pub mod ws;

use crate::auth::store::AuthStore;
use crate::auth::waiter::AuthWaiter;
use crate::config::Config;
use crate::jobs::store::TaskStore;
use crate::models::{ModelProviderStore, ModelStore};
use crate::project::ProjectStore;
use crate::provider::ProviderRegistry;
use crate::service::ServiceStore;
use crate::workspace::Workspace;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub task_store: Arc<TaskStore>,
    pub project_store: Arc<ProjectStore>,
    pub service_store: ServiceStore,
    pub model_store: ModelStore,
    pub model_provider_store: ModelProviderStore,
    pub workspace: Arc<Workspace>,
    pub providers: ProviderRegistry,
    pub auth_store: Arc<AuthStore>,
    pub auth_waiter: AuthWaiter,
}
