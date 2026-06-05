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

    pub fn branch_dir(
        &self,
        service_slug: &str,
        project_slug: &str,
        branch_slug: &str,
    ) -> PathBuf {
        self.project_root(service_slug, project_slug).join(branch_slug)
    }

    /// Stable path for hooks shared across every project worktree. Lives
    /// outside any project dir so it is never confused with a service slug
    /// (the leading dot blocks both git and provider-slug collisions).
    pub fn shared_hooks_dir(&self) -> PathBuf {
        self.base.join(".agent-hooks")
    }

    pub fn authcheck_hook_path(&self) -> PathBuf {
        self.shared_hooks_dir().join("authcheck.sh")
    }

    /// Materialise the bundled authcheck hook script under
    /// `<repo_base>/.agent-hooks/`. Idempotent; rewrites on every startup so
    /// agent upgrades take effect even when the workspace base persists.
    pub async fn install_shared_hooks(&self) -> Result<()> {
        const AUTHCHECK_SH: &str =
            include_str!("../../defaults/.claude/hooks/authcheck.sh");

        let dir = self.shared_hooks_dir();
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;

        let path = self.authcheck_hook_path();
        tokio::fs::write(&path, AUTHCHECK_SH)
            .await
            .with_context(|| format!("writing {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = tokio::fs::metadata(&path).await?;
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            tokio::fs::set_permissions(&path, perms).await?;
        }
        Ok(())
    }

    /// Acquire both the in-process mutex and the cross-process advisory lock
    /// guarding mutations of the project's branch worktrees.
    pub async fn lock_project(
        &self,
        service_slug: &str,
        project_slug: &str,
    ) -> Result<ProjectLockGuard> {
        let key = format!("{service_slug}/{project_slug}");
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
        let lock_path = project_root.join(".lock");

        let file_lock = tokio::task::spawn_blocking(move || AdvisoryFileLock::acquire(&lock_path))
            .await
            .context("spawn_blocking failed")??;

        Ok(ProjectLockGuard {
            _in_proc,
            _file: file_lock,
        })
    }

    /// Idempotent: clones if missing, otherwise fetches and resets the branch.
    pub async fn clone_or_fetch(
        &self,
        path: &Path,
        auth_url: &str,
        branch: &str,
    ) -> Result<()> {
        git::clone_or_fetch(path, auth_url, branch).await
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

pub struct ProjectLockGuard {
    _in_proc: tokio::sync::OwnedMutexGuard<()>,
    _file: AdvisoryFileLock,
}
