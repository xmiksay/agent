//! Resolve the node toolchain NVM would activate, by reading the NVM filesystem
//! directly (no shell sourcing). The runner prepends the resolved `bin` dir to
//! the agent's `PATH` so the `claude` CLI and every command it spawns see node.
//!
//! NVM's only runtime effect that matters here is putting
//! `<NVM_DIR>/versions/node/<version>/bin` at the front of `PATH`; we compute
//! that dir and let the runner set it. Every failure is logged and yields
//! `None` so a misconfigured NVM never blocks a run.

use std::path::{Path, PathBuf};

use tokio::fs;
use tracing::warn;

/// Resolve the node `bin` dir NVM would activate, honouring the worktree's
/// `.nvmrc` when present, else the NVM `default` alias. Returns `None` (logged)
/// on any failure so a misconfigured NVM never blocks the run.
pub async fn resolve_node_bin(nvm_dir: &Path, work_dir: &Path) -> Option<PathBuf> {
    if !nvm_dir.is_dir() {
        warn!(nvm_dir = %nvm_dir.display(), "NVM_DIR is not a directory; skipping NVM activation");
        return None;
    }

    // Requested version: the worktree's `.nvmrc` (trimmed) wins; otherwise the
    // `default` alias. A `.nvmrc` requesting an uninstalled version falls back to
    // `default` rather than failing the run.
    let from_nvmrc = read_trimmed(&work_dir.join(".nvmrc")).await;
    if let Some(req) = from_nvmrc.as_deref() {
        if let Some(bin) = resolve_request(nvm_dir, req).await {
            return Some(bin);
        }
        warn!(
            requested = req,
            "`.nvmrc` version not installed under NVM; falling back to the `default` alias"
        );
    }

    match read_alias(nvm_dir, "default").await {
        Some(target) => {
            let bin = resolve_request(nvm_dir, &target).await;
            if bin.is_none() {
                warn!(
                    target,
                    "NVM `default` alias points at an uninstalled version"
                );
            }
            bin
        }
        None => {
            warn!("NVM has no `default` alias and the worktree has no usable `.nvmrc`");
            None
        }
    }
}

/// Resolve a single version request (`vX.Y.Z`, a partial like `20`/`v20`, or an
/// alias name) to its `bin` dir if a matching version is installed.
async fn resolve_request(nvm_dir: &Path, req: &str) -> Option<PathBuf> {
    let req = req.trim();
    if req.is_empty() {
        return None;
    }

    // An alias (e.g. `lts/*`, a custom name) → follow it, then re-resolve. Guard
    // against a self-referential alias by not recursing on an identical target.
    if let Some(target) = read_alias(nvm_dir, req).await
        && target != req
        && let Some(bin) = Box::pin(resolve_request(nvm_dir, &target)).await
    {
        return Some(bin);
    }

    let installed = installed_versions(nvm_dir).await;
    let resolved = match_version(req, &installed)?;
    let bin = nvm_dir.join("versions/node").join(&resolved).join("bin");
    bin.is_dir().then_some(bin)
}

/// Pick the installed version matching `req`: an exact `vX.Y.Z` match, else the
/// highest installed version sharing the requested `vMAJOR[.MINOR]` prefix
/// (NVM's "highest matching" rule). `req` may omit the leading `v`.
fn match_version(req: &str, installed: &[String]) -> Option<String> {
    let want = req.strip_prefix('v').unwrap_or(req);

    // Exact match first (`v20.11.1` or `20.11.1`).
    if let Some(exact) = installed.iter().find(|v| v.trim_start_matches('v') == want) {
        return Some(exact.clone());
    }

    // Partial prefix (`20` or `20.11`): match on dot-delimited components so `20`
    // matches `v20.*` but not `v200.*`. Pick the numerically highest.
    let want_parts: Vec<&str> = want.split('.').collect();
    installed
        .iter()
        .filter(|v| {
            let have: Vec<&str> = v.trim_start_matches('v').split('.').collect();
            want_parts.len() <= have.len() && want_parts.iter().zip(&have).all(|(a, b)| a == b)
        })
        .max_by(|a, b| cmp_semver(a, b))
        .cloned()
}

/// Compare two `vX.Y.Z` strings numerically (component-wise).
fn cmp_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a).cmp(&parse(b))
}

/// List concrete installed versions (the directory names under
/// `<nvm_dir>/versions/node/`, e.g. `v20.11.1`).
async fn installed_versions(nvm_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let dir = nvm_dir.join("versions/node");
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return out;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(name) = entry.file_name().into_string() {
            out.push(name);
        }
    }
    out
}

/// Read `<nvm_dir>/alias/<name>`, trimmed, if it exists.
async fn read_alias(nvm_dir: &Path, name: &str) -> Option<String> {
    read_trimmed(&nvm_dir.join("alias").join(name)).await
}

/// Read a file and trim surrounding whitespace; `None` if unreadable or empty.
async fn read_trimmed(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).await.ok()?;
    let trimmed = raw.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    /// A unique temp directory removed on drop — avoids a `tempfile` dependency
    /// for the few filesystem-shaped tests here.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("agent-nvm-test-{}", uuid::Uuid::new_v4()));
            stdfs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = stdfs::remove_dir_all(&self.0);
        }
    }

    /// Build a fake NVM layout with the given installed versions, optional
    /// `default` alias, and an optional `.nvmrc` in a separate work dir.
    fn fake_nvm(versions: &[&str], default_alias: Option<&str>) -> TempDir {
        let dir = TempDir::new();
        for v in versions {
            let bin = dir.path().join("versions/node").join(v).join("bin");
            stdfs::create_dir_all(&bin).unwrap();
        }
        if let Some(target) = default_alias {
            let alias_dir = dir.path().join("alias");
            stdfs::create_dir_all(&alias_dir).unwrap();
            stdfs::write(alias_dir.join("default"), format!("{target}\n")).unwrap();
        }
        dir
    }

    fn work_with_nvmrc(content: Option<&str>) -> TempDir {
        let dir = TempDir::new();
        if let Some(c) = content {
            stdfs::write(dir.path().join(".nvmrc"), c).unwrap();
        }
        dir
    }

    #[tokio::test]
    async fn nvmrc_exact_version_wins() {
        let nvm = fake_nvm(&["v18.20.0", "v20.11.1"], Some("18"));
        let work = work_with_nvmrc(Some("v20.11.1\n"));
        let bin = resolve_node_bin(nvm.path(), work.path()).await.unwrap();
        assert!(bin.ends_with("versions/node/v20.11.1/bin"));
    }

    #[tokio::test]
    async fn nvmrc_partial_picks_highest_matching() {
        let nvm = fake_nvm(&["v20.9.0", "v20.11.1", "v22.0.0"], Some("18"));
        let work = work_with_nvmrc(Some("20"));
        let bin = resolve_node_bin(nvm.path(), work.path()).await.unwrap();
        assert!(bin.ends_with("versions/node/v20.11.1/bin"));
    }

    #[tokio::test]
    async fn falls_back_to_default_when_no_nvmrc() {
        let nvm = fake_nvm(&["v18.20.0", "v20.11.1"], Some("v18.20.0"));
        let work = work_with_nvmrc(None);
        let bin = resolve_node_bin(nvm.path(), work.path()).await.unwrap();
        assert!(bin.ends_with("versions/node/v18.20.0/bin"));
    }

    #[tokio::test]
    async fn default_alias_partial_resolves() {
        let nvm = fake_nvm(&["v18.20.0", "v18.19.0"], Some("18"));
        let work = work_with_nvmrc(None);
        let bin = resolve_node_bin(nvm.path(), work.path()).await.unwrap();
        assert!(bin.ends_with("versions/node/v18.20.0/bin"));
    }

    #[tokio::test]
    async fn nvmrc_uninstalled_falls_back_to_default() {
        let nvm = fake_nvm(&["v18.20.0"], Some("18"));
        let work = work_with_nvmrc(Some("20"));
        let bin = resolve_node_bin(nvm.path(), work.path()).await.unwrap();
        assert!(bin.ends_with("versions/node/v18.20.0/bin"));
    }

    #[tokio::test]
    async fn none_when_nothing_resolves() {
        let nvm = fake_nvm(&["v18.20.0"], None);
        let work = work_with_nvmrc(Some("20"));
        assert!(resolve_node_bin(nvm.path(), work.path()).await.is_none());
    }

    #[tokio::test]
    async fn none_when_nvm_dir_missing() {
        let work = work_with_nvmrc(None);
        let missing = work.path().join("does-not-exist");
        assert!(resolve_node_bin(&missing, work.path()).await.is_none());
    }
}
