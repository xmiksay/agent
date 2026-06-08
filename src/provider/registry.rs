use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use tokio::sync::RwLock;
use tracing::warn;
use uuid::Uuid;

use crate::git_service::{GitService, GitServiceStore};
use crate::project::ProviderKind;
use crate::provider::GitProvider;
use crate::provider::github::GitHubClient;
use crate::provider::gitlab::GitLabClient;

/// Cache of provider clients keyed by `git_service.id`. Rebuilt whenever a
/// service is added, updated, or removed.
#[derive(Clone)]
pub struct ProviderRegistry {
    store: GitServiceStore,
    by_id: Arc<RwLock<HashMap<Uuid, Entry>>>,
}

struct Entry {
    service: GitService,
    client: Arc<dyn GitProvider>,
}

impl ProviderRegistry {
    pub fn new(store: GitServiceStore) -> Self {
        Self {
            store,
            by_id: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load all services from the database and rebuild the cache.
    pub async fn reload(&self) -> Result<()> {
        let services = self.store.list().await.context("listing git_services")?;
        let mut map = HashMap::with_capacity(services.len());
        for svc in services {
            match build_client(&svc) {
                Ok(client) => {
                    map.insert(
                        svc.id,
                        Entry {
                            service: svc,
                            client,
                        },
                    );
                }
                Err(e) => {
                    warn!(slug = %svc.slug, error = %e, "skipping git_service: cannot build client");
                }
            }
        }
        let mut guard = self.by_id.write().await;
        *guard = map;
        Ok(())
    }

    pub async fn get(&self, service_id: Uuid) -> Option<Arc<dyn GitProvider>> {
        let guard = self.by_id.read().await;
        guard.get(&service_id).map(|e| e.client.clone())
    }

    pub async fn require(&self, service_id: Uuid) -> Result<Arc<dyn GitProvider>> {
        self.get(service_id)
            .await
            .ok_or_else(|| anyhow!("no git_service configured for {service_id}"))
    }

    pub async fn service(&self, service_id: Uuid) -> Option<GitService> {
        let guard = self.by_id.read().await;
        guard.get(&service_id).map(|e| e.service.clone())
    }

    /// Refresh a single service from the store; pass `None` after deletion.
    pub async fn refresh(&self, service_id: Uuid) -> Result<()> {
        let svc = self.store.get(service_id).await?;
        let mut guard = self.by_id.write().await;
        match svc {
            Some(svc) => {
                let client = build_client(&svc)?;
                guard.insert(
                    service_id,
                    Entry {
                        service: svc,
                        client,
                    },
                );
            }
            None => {
                guard.remove(&service_id);
            }
        }
        Ok(())
    }
}

fn build_client(svc: &GitService) -> Result<Arc<dyn GitProvider>> {
    let creds = svc.credentials()?;
    Ok(match svc.kind {
        ProviderKind::Gitlab => {
            Arc::new(GitLabClient::new(svc.id, &svc.base_url, creds)) as Arc<dyn GitProvider>
        }
        ProviderKind::Github => {
            Arc::new(GitHubClient::new(svc.id, &svc.base_url, creds)) as Arc<dyn GitProvider>
        }
    })
}
