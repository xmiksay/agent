use anyhow::Result;

use crate::jobs::types::ClaudeOutput;

pub mod claude;

pub use claude::ClaudeCode;

/// A `can_use_tool` permission prompt parsed off the agent's stdout control
/// protocol. `input` is the tool's verbatim input object (e.g. `{"command": …}`
/// for Bash) — echoed back unchanged on an Allow decision.
pub struct PermissionRequest {
    pub request_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

/// The operator/policy decision for a [`PermissionRequest`], encoded back onto
/// the agent's stdin as a `control_response`.
#[derive(Debug, PartialEq)]
pub enum PermissionDecision {
    /// Permit the tool call. `updated_input` must echo the original input
    /// verbatim — the CLI rejects an allow without it.
    Allow { updated_input: serde_json::Value },
    /// Reject the tool call (or, for AskUserQuestion, deliver `message` as the
    /// operator's answer).
    Deny { message: String },
}

/// A coding-agent CLI backend. Only Claude Code is wired up today; the trait
/// isolates every agent-specific decision (invocation, control-protocol
/// encoding, output parsing) so another CLI can be added — and, later, selected
/// per task — without touching the runner.
///
/// All methods are synchronous and pure, so they're easy to unit-test.
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

    /// Parse a `can_use_tool` permission request off a single stdout line.
    /// Returns `None` for any line that isn't such a request.
    fn parse_permission_request(&self, line: &str) -> Option<PermissionRequest>;

    /// Encode a permission decision as a single stdin `control_response` line.
    fn encode_permission_response(&self, request_id: &str, decision: &PermissionDecision)
    -> String;

    /// Parse the final normalized result from the process's full stdout.
    fn parse_result(&self, stdout: &str) -> Result<ClaudeOutput>;

    /// Sniff a session id from a single streamed stdout line, if present.
    fn extract_session_id(&self, line: &str) -> Option<String>;

    /// Sniff a cumulative output-token delta from a single streamed line.
    fn extract_output_tokens(&self, line: &str) -> Option<u64>;
}
