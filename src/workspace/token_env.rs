//! The mutable per-worktree `agent.env` the agent re-sources on every command.
//!
//! The `claude` child's process env is frozen at spawn, so the `GH_TOKEN` /
//! `GITLAB_TOKEN` baked in once cannot be rotated in place. A GitHub App
//! installation token only lives ~1h, so a warm-idle session or a single
//! long turn outlives it and the agent's own `git`/`gh`/`glab` calls start
//! failing. The fix: spawn the agent with `BASH_ENV=<work_dir>/.git/agent.env`
//! (bash sources it at the start of every non-interactive shell — exactly how
//! the CLI's Bash tool runs commands) and refresh the file's *contents* with a
//! fresh token. The path stays fixed for the process; only the bytes change —
//! that is what makes mid-session rotation possible.
//!
//! The file lives under `.git/` so it is never part of the working tree, the
//! branch diff, or a commit. It holds a live short-lived token at `0600`,
//! outside `.git/config` — consistent with the #22 "no secret in `.git/config`"
//! rule.

use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Stable path of the agent's env file for a worktree: `<work_dir>/.git/agent.env`.
/// `BASH_ENV` points here for the spawned agent.
pub fn agent_env_path(work_dir: &Path) -> PathBuf {
    work_dir.join(".git").join("agent.env")
}

/// (Re)write `<work_dir>/.git/agent.env` with `export <token_var>='<token>'` at
/// mode `0600`, atomically (write a unique temp sibling, then rename). Reused by
/// the runner (at spawn and per turn) and the operator refresh endpoint.
pub async fn write_agent_env(work_dir: &Path, token_var: &str, token: &str) -> Result<()> {
    let path = agent_env_path(work_dir);
    // Single-quote so shell metacharacters in the token stay literal; an
    // embedded `'` is closed, escaped, and reopened (`'\''`).
    let contents = format!("export {token_var}={}\n", shell_single_quote(token));
    // Unique temp name so a per-turn rewrite and an operator refresh can't
    // collide on the same sibling; rename is atomic, so readers always see one
    // complete file.
    let tmp = path.with_file_name(format!("agent.env.{}.tmp", uuid::Uuid::new_v4()));

    tokio::task::spawn_blocking(move || -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("opening {}", tmp.display()))?;
        f.write_all(contents.as_bytes())
            .with_context(|| format!("writing {}", tmp.display()))?;
        // Rename preserves the temp file's 0600 mode and replaces any prior file.
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    })
    .await
    .context("spawn_blocking for agent.env write")?
}

/// Wrap a value in single quotes for a POSIX shell, escaping any embedded quote.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_worktree() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-env-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".git")).expect("mkdir .git");
        dir
    }

    #[tokio::test]
    async fn writes_under_git_at_0600_with_export_line() {
        let work = temp_worktree();
        write_agent_env(&work, "GH_TOKEN", "ghs_abc123")
            .await
            .expect("write");

        let path = agent_env_path(&work);
        assert_eq!(path, work.join(".git").join("agent.env"));

        let body = std::fs::read_to_string(&path).expect("read");
        assert_eq!(body, "export GH_TOKEN='ghs_abc123'\n");

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "must be 0600, got {:o}", mode & 0o777);

        let _ = std::fs::remove_dir_all(&work);
    }

    #[tokio::test]
    async fn rewrite_replaces_atomically_and_escapes_quotes() {
        let work = temp_worktree();
        write_agent_env(&work, "GITLAB_TOKEN", "old")
            .await
            .expect("first");
        // A token carrying a single quote must round-trip as a literal.
        write_agent_env(&work, "GITLAB_TOKEN", "a'b")
            .await
            .expect("second");

        let body = std::fs::read_to_string(agent_env_path(&work)).expect("read");
        assert_eq!(body, "export GITLAB_TOKEN='a'\\''b'\n");

        // No temp files left behind after an atomic replace.
        let leftovers: Vec<_> = std::fs::read_dir(work.join(".git"))
            .expect("readdir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&work);
    }
}
