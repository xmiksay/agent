//! The queue catalog: CRUD over the `queues` table. A queue is just an
//! operator-orderable backlog bucket (`name` + queue-vs-queue `priority`); tasks
//! reference it via `tasks.queue_id`. Carries no secrets, so the entity `Model`
//! doubles as the API view. The scheduling logic lives in `jobs::queue`.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use sea_orm::*;
use serde::Deserialize;
use uuid::Uuid;

use crate::entity::queues;

#[derive(Clone, Debug, Deserialize)]
pub struct NewQueue {
    pub name: String,
    #[serde(default)]
    pub priority: i16,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdateQueue {
    pub name: Option<String>,
    pub priority: Option<i16>,
}

#[derive(Clone)]
pub struct QueueStore {
    db: DatabaseConnection,
}

impl QueueStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Queues ordered by `priority` (highest first), then name — the same order
    /// the scheduler resolves them in.
    pub async fn list(&self) -> Result<Vec<queues::Model>> {
        queues::Entity::find()
            .order_by_desc(queues::Column::Priority)
            .order_by_asc(queues::Column::Name)
            .all(&self.db)
            .await
            .context("failed to list queues")
    }

    pub async fn get(&self, id: Uuid) -> Result<Option<queues::Model>> {
        queues::Entity::find_by_id(id)
            .one(&self.db)
            .await
            .context("failed to load queue")
    }

    pub async fn create(&self, new: NewQueue) -> Result<queues::Model> {
        let name = new.name.trim().to_string();
        if name.is_empty() {
            bail!("name is required");
        }
        let id = Uuid::new_v4();
        let active = queues::ActiveModel {
            id: Set(id),
            name: Set(name),
            priority: Set(new.priority),
            created_at: Set(Utc::now().into()),
        };
        queues::Entity::insert(active)
            .exec(&self.db)
            .await
            .context("failed to insert queue")?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("queue disappeared after insert"))
    }

    pub async fn update(&self, id: Uuid, upd: UpdateQueue) -> Result<queues::Model> {
        let mut active: queues::ActiveModel = queues::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("queue not found"))?
            .into();
        if let Some(name) = upd.name {
            let name = name.trim().to_string();
            if name.is_empty() {
                bail!("name must not be empty");
            }
            active.name = Set(name);
        }
        if let Some(priority) = upd.priority {
            active.priority = Set(priority);
        }
        active
            .update(&self.db)
            .await
            .context("failed to update queue")?;
        self.get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("queue disappeared after update"))
    }

    /// Delete a queue. The `tasks.queue_id` FK is `ON DELETE SET NULL`, so its
    /// tasks are un-queued rather than blocked.
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        let res = queues::Entity::delete_by_id(id)
            .exec(&self.db)
            .await
            .context("failed to delete queue")?;
        if res.rows_affected == 0 {
            bail!("queue not found");
        }
        Ok(())
    }
}
