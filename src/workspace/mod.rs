pub mod git;
pub mod layout;
pub mod lock;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use dashmap::DashMap;
use tokio::sync::Mutex;

use lock::AdvisoryFileLock;

/// Thin disk-only layer. All metadata lives in [`crate::project::ProjectStore`].
#[derive(Clone)]
pub struct Workspace {
    base: PathBuf,
    in_proc_locks: Arc<DashMap<String, Arc<Mutex<()>>>>,
}

impl Workspace {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            in_proc_locks: Arc::new(DashMap::new()),
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn project_root(&self, service_slug: &str, project_slug: &str) -> PathBuf {
        self.base.join(service_slug).join(project_slug)
    }

    pub fn branch_dir(&self, service_slug: &str, project_slug: &str, branch_slug: &str) -> PathBuf {
        self.project_root(service_slug, project_slug)
            .join(branch_slug)
    }

    /// Acquire both the in-process mutex and the cross-process advisory lock
    /// guarding mutations of a single branch worktree. Scoped per branch (not
    /// per project) so tasks on different branches of the same project — each
    /// of which has its own independent clone — can set up concurrently.
    pub async fn lock_branch(
        &self,
        service_slug: &str,
        project_slug: &str,
        branch_slug: &str,
    ) -> Result<BranchLockGuard> {
        let key = format!("{service_slug}/{project_slug}/{branch_slug}");
        let mutex = self
            .in_proc_locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _in_proc = mutex.lock_owned().await;

        let project_root = self.project_root(service_slug, project_slug);
        tokio::fs::create_dir_all(&project_root)
            .await
            .with_context(|| format!("creating {}", project_root.display()))?;
        // Sibling file of the branch worktree dir, so removing the worktree
        // doesn't disturb a held lock.
        let lock_path = project_root.join(format!("{branch_slug}.lock"));

        let file_lock = tokio::task::spawn_blocking(move || AdvisoryFileLock::acquire(&lock_path))
            .await
            .context("spawn_blocking failed")??;

        Ok(BranchLockGuard {
            _in_proc,
            _file: file_lock,
        })
    }

    /// Idempotent: clones if missing, otherwise fetches and checks out the
    /// branch. Authenticates over token-HTTPS (see [`git::HttpsAuth`]).
    pub async fn clone_or_fetch(
        &self,
        path: &Path,
        auth: &git::HttpsAuth,
        branch: &str,
        default_branch: &str,
    ) -> Result<()> {
        git::clone_or_fetch(path, auth, branch, default_branch).await
    }

    /// Remove only this branch's working tree (NOT any sibling branches).
    pub async fn remove_branch_dir(
        &self,
        service_slug: &str,
        project_slug: &str,
        branch_slug: &str,
    ) -> Result<()> {
        let dir = self.branch_dir(service_slug, project_slug, branch_slug);
        if tokio::fs::metadata(&dir).await.is_err() {
            return Ok(());
        }
        tokio::fs::remove_dir_all(&dir)
            .await
            .with_context(|| format!("removing {}", dir.display()))?;
        Ok(())
    }
}

pub struct BranchLockGuard {
    _in_proc: tokio::sync::OwnedMutexGuard<()>,
    _file: AdvisoryFileLock,
}
