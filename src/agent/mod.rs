use std::path::{Path, PathBuf};

use anyhow::Result;
use uuid::Uuid;

use crate::jobs::types::ClaudeOutput;

pub mod claude;

pub use claude::ClaudeCode;

/// A file the backend wants materialized in the worktree before a run — agent
/// config, permission hooks, etc. `rel_path` is relative to the worktree root.
pub struct WorktreeFile {
    pub rel_path: PathBuf,
    pub contents: String,
}

/// A coding-agent CLI backend. Only Claude Code is wired up today; the trait
/// isolates every agent-specific decision (invocation, config files, output
/// parsing) so another CLI can be added — and, later, selected per task —
/// without touching the runner.
///
/// All methods are synchronous: any filesystem work (writing `worktree_files`)
/// is done by the runner so backends stay pure and easy to unit-test.
pub trait AgentBackend: Send + Sync {
    /// CLI program name, e.g. `claude`.
    fn program(&self) -> &str;

    /// Short identifier for this backend, surfaced in the live-stream envelope
    /// so clients know which agent produced an event (e.g. `claude`).
    fn name(&self) -> &str;

    /// Args for an interactive session: bidirectional stream-json plus an
    /// optional resume session id. The initial prompt and any follow-up operator
    /// messages are written to the process's stdin (see `encode_user_message`),
    /// not passed as args.
    fn build_args(&self, resume_session_id: Option<&str>) -> Vec<String>;

    /// Encode one operator message as a single stdin line in the backend's
    /// streaming-input format.
    fn encode_user_message(&self, text: &str) -> String;

    /// Extra env vars for the spawned process. The provider PAT is injected by
    /// the runner separately since it's agent-agnostic.
    fn extra_env(&self, task_id: Uuid, agent_port: &str) -> Vec<(String, String)>;

    /// Files to write into the worktree before the run (config + hooks).
    fn worktree_files(&self, authcheck_hook: &Path) -> Vec<WorktreeFile>;

    /// Parse the final normalized result from the process's full stdout.
    fn parse_result(&self, stdout: &str) -> Result<ClaudeOutput>;

    /// Sniff a session id from a single streamed stdout line, if present.
    fn extract_session_id(&self, line: &str) -> Option<String>;

    /// Sniff a cumulative output-token delta from a single streamed line.
    fn extract_output_tokens(&self, line: &str) -> Option<u64>;
}
