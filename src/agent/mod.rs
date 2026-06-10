use std::sync::Arc;

use anyhow::{Result, bail};

use crate::jobs::types::ClaudeOutput;
use crate::models::ResolvedModel;

pub mod claude;

pub use claude::ClaudeCode;

/// Every system-defined provider `kind` that has a wired backend — the choices
/// a provider row's `kind` may take. Keep in sync with `backend_for`.
pub const KNOWN_PROVIDER_KINDS: [&str; 1] = ["claude_code"];

/// Resolve a provider's system-defined `kind` to its agent backend. This is the
/// seam the issue calls "provider resolved from model, run correct command": the
/// model's provider row names a kind, and this picks the CLI that runs it. Only
/// `claude_code` is wired today.
pub fn backend_for(kind: &str) -> Result<Arc<dyn AgentBackend>> {
    match kind {
        "claude_code" => Ok(Arc::new(ClaudeCode)),
        other => bail!("no agent backend for provider kind '{other}'"),
    }
}

/// Resolve a selected catalog model into its backend plus the `model_id` arg the
/// CLI is given. `None` (no model configured) → the default backend and no
/// explicit model, so the CLI uses its own default.
pub fn resolve_backend(
    model: Option<&ResolvedModel>,
) -> Result<(Arc<dyn AgentBackend>, Option<String>)> {
    let kind = model
        .map(|m| m.provider_kind.as_str())
        .unwrap_or("claude_code");
    Ok((backend_for(kind)?, model.map(|m| m.model_id.clone())))
}

/// Apply a resolved model's provider environment (API key + base-URL override, in
/// the backend-specific var names) to the agent spawn command. A `None` model or
/// unset fields leave the CLI on its subscription login + default host.
pub fn apply_model_env(
    cmd: &mut tokio::process::Command,
    backend: &dyn AgentBackend,
    model: Option<&ResolvedModel>,
) {
    let Some(m) = model else { return };
    if let Some(key) = m.api_key.as_deref() {
        cmd.env(backend.api_key_env(), key);
    }
    if let Some(url) = m.api_url.as_deref() {
        cmd.env(backend.base_url_env(), url);
    }
}

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

    /// Environment variable that carries the provider's API key when the provider
    /// is configured to run in API mode (rather than on a subscription login).
    fn api_key_env(&self) -> &str;

    /// Environment variable that overrides the provider's API base URL (e.g. a
    /// self-hosted / Ollama-style endpoint), when the provider sets one.
    fn base_url_env(&self) -> &str;

    /// Args for an interactive session: bidirectional stream-json plus an
    /// optional resume session id and an optional `model` (the catalog model's
    /// `model_id`, passed to the CLI's model flag; `None` lets the CLI use its
    /// own default). When `unbound` is true the backend runs **without permission
    /// gating** (every tool call allowed, no operator approval) — a dangerous mode
    /// the model opts into. The initial prompt and follow-up operator messages are
    /// written to the process's stdin (see `encode_user_message`), not as args.
    fn build_args(
        &self,
        resume_session_id: Option<&str>,
        model: Option<&str>,
        unbound: bool,
    ) -> Vec<String>;

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
