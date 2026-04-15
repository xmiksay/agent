pub use sea_orm_migration::prelude::*;

mod m20260415_000001_create_tasks;
mod m20260415_000002_create_task_results;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260415_000001_create_tasks::Migration),
            Box::new(m20260415_000002_create_task_results::Migration),
        ]
    }
}
