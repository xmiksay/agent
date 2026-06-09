pub use sea_orm_migration::prelude::*;

mod m20260415_000001_create_tasks;
mod m20260415_000002_create_task_results;
mod m20260601_000003_create_projects;
mod m20260601_000004_create_project_branches;
mod m20260601_000005_extend_tasks;
mod m20260601_000006_create_auth_requests;
mod m20260601_000007_create_git_services;
mod m20260601_000008_add_git_service_to_tasks;
mod m20260601_000009_add_task_session_id;
mod m20260601_000010_add_task_pid;
mod m20260601_000011_add_auth_request_metadata;
mod m20260605_000012_add_task_pending_message;
mod m20260605_000013_create_task_events;
mod m20260606_000014_add_project_env_vars;
mod m20260607_000015_add_git_service_autofire;
mod m20260607_000016_add_task_event_kind;
mod m20260608_000017_split_task_state;
mod m20260608_000018_add_git_service_app_auth;
mod m20260608_000019_drop_one_github_index;
mod m20260608_000020_add_git_service_trigger;
mod m20260609_000021_rename_git_services_to_service;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260415_000001_create_tasks::Migration),
            Box::new(m20260415_000002_create_task_results::Migration),
            Box::new(m20260601_000003_create_projects::Migration),
            Box::new(m20260601_000004_create_project_branches::Migration),
            Box::new(m20260601_000005_extend_tasks::Migration),
            Box::new(m20260601_000006_create_auth_requests::Migration),
            Box::new(m20260601_000007_create_git_services::Migration),
            Box::new(m20260601_000008_add_git_service_to_tasks::Migration),
            Box::new(m20260601_000009_add_task_session_id::Migration),
            Box::new(m20260601_000010_add_task_pid::Migration),
            Box::new(m20260601_000011_add_auth_request_metadata::Migration),
            Box::new(m20260605_000012_add_task_pending_message::Migration),
            Box::new(m20260605_000013_create_task_events::Migration),
            Box::new(m20260606_000014_add_project_env_vars::Migration),
            Box::new(m20260607_000015_add_git_service_autofire::Migration),
            Box::new(m20260607_000016_add_task_event_kind::Migration),
            Box::new(m20260608_000017_split_task_state::Migration),
            Box::new(m20260608_000018_add_git_service_app_auth::Migration),
            Box::new(m20260608_000019_drop_one_github_index::Migration),
            Box::new(m20260608_000020_add_git_service_trigger::Migration),
            Box::new(m20260609_000021_rename_git_services_to_service::Migration),
        ]
    }
}
