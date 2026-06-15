//! Per-turn watchdog helpers, split out of `runner.rs` to keep it under the
//! 400-line cap: SIGKILL the agent's whole process group when a turn outruns
//! `JOB_TIMEOUT_SECS`, and the pure terminal-state mapping.

use tokio::process::Child;

/// SIGKILL the agent's entire process group, then reap the leader.
///
/// The CLI is spawned as its own process-group leader (`process_group(0)`), so
/// its pid doubles as the pgid and `kill(-pgid)` reaches every descendant — the
/// orphaned grandchildren (a backgrounded `cargo test`, hung test binaries) that
/// killing the direct child alone would leave running. That leak was the actual
/// incident this guards against, so the group kill is the whole point.
pub(crate) async fn kill_process_group(child: &mut Child) {
    if let Some(pid) = child.id() {
        // SAFETY: `kill(2)` with a negative pid signals the process group; no
        // Rust aliasing/lifetime invariants are involved. Best-effort — a gone
        // group (ESRCH) is ignored.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    // Reap the leader so it isn't left a zombie; the group kill already
    // delivered the signal, this just collects the status.
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// Pure terminal-state mapping: how a session's end → `(agent_state, task_state,
/// note)`. A per-turn timeout or a token-budget kill is a *resumable* failure —
/// `finish_task` never clears `session_id`, so the operator can Resume; a clean
/// exit completes, a non-zero exit fails.
pub(crate) fn final_disposition(
    killed_for_timeout: bool,
    killed_for_budget: bool,
    exit_ok: bool,
) -> (&'static str, &'static str, Option<&'static str>) {
    if killed_for_timeout {
        ("failed", "failed", Some("killed: per-turn timeout"))
    } else if killed_for_budget {
        ("failed", "failed", Some("killed: token budget"))
    } else if exit_ok {
        ("cold", "completed", None)
    } else {
        ("failed", "failed", Some("agent exited non-zero"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::process::Command;
    use uuid::Uuid;

    #[test]
    fn disposition_timeout_takes_precedence_and_is_resumable() {
        assert_eq!(
            final_disposition(true, false, false),
            ("failed", "failed", Some("killed: per-turn timeout"))
        );
        // Timeout wins even if other flags are set.
        assert_eq!(
            final_disposition(true, true, true),
            ("failed", "failed", Some("killed: per-turn timeout"))
        );
    }

    #[test]
    fn disposition_budget_and_exit_paths() {
        assert_eq!(
            final_disposition(false, true, false),
            ("failed", "failed", Some("killed: token budget"))
        );
        assert_eq!(
            final_disposition(false, false, true),
            ("cold", "completed", None)
        );
        assert_eq!(
            final_disposition(false, false, false),
            ("failed", "failed", Some("agent exited non-zero"))
        );
    }

    fn proc_alive(pid: i32) -> bool {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    /// The real point of the guard: a grandchild the agent backgrounded must die
    /// when the turn is killed. Spawn a shell in its own group that backgrounds a
    /// long `sleep`, record the grandchild pid, kill the group, assert it's gone —
    /// proving the kill reaches past the direct child.
    #[tokio::test]
    async fn kill_process_group_reaps_grandchildren() {
        let dir = std::env::temp_dir().join(format!("agent-kill-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let pidfile = dir.join("gc.pid");

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(format!("sleep 120 & echo $! > {}; wait", pidfile.display()))
            .process_group(0)
            .spawn()
            .expect("spawn sh");

        let gc_pid: i32 = loop {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if let Ok(p) = s.trim().parse() {
                    break p;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(proc_alive(gc_pid), "grandchild alive before kill");

        kill_process_group(&mut child).await;

        let mut gone = false;
        for _ in 0..100 {
            if !proc_alive(gc_pid) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gone, "grandchild must die with the process group");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
