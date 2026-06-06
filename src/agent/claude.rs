use anyhow::{Context, Result};

use crate::agent::{AgentBackend, PermissionDecision, PermissionRequest};
use crate::jobs::types::ClaudeOutput;

/// The Claude Code CLI backend (`claude`). Drives it headless with
/// `--output-format stream-json` and parses the trailing `result` event.
pub struct ClaudeCode;

impl AgentBackend for ClaudeCode {
    fn program(&self) -> &str {
        "claude"
    }

    fn name(&self) -> &str {
        "claude"
    }

    fn build_args(&self, resume_session_id: Option<&str>) -> Vec<String> {
        // Interactive headless session: stream-json on both ends keeps the
        // process alive, reading operator messages from stdin and emitting
        // newline-delimited JSON events on stdout. `--replay-user-messages`
        // echoes each stdin message back on stdout so it shows up in the live
        // timeline. Each turn ends with a `{"type":"result", ...}` event; the
        // process exits when stdin closes (EOF) — our graceful Stop.
        //
        // `--permission-mode default --permission-prompt-tool stdio` routes
        // tool-permission prompts over the stream-json control protocol: the CLI
        // emits a `control_request`/`can_use_tool` on stdout and waits for our
        // `control_response` on stdin. That replaces the old PreToolUse hook —
        // trivially-safe tools are auto-allowed, everything else prompts us.
        let mut args = vec!["--print".to_string()];
        if let Some(sid) = resume_session_id {
            args.push("-r".to_string());
            args.push(sid.to_string());
        }
        args.extend(
            [
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--replay-user-messages",
                "--permission-mode",
                "default",
                "--permission-prompt-tool",
                "stdio",
            ]
            .map(String::from),
        );
        args
    }

    fn encode_user_message(&self, text: &str) -> String {
        serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
        })
        .to_string()
    }

    fn parse_permission_request(&self, line: &str) -> Option<PermissionRequest> {
        let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        if v.get("type").and_then(|t| t.as_str()) != Some("control_request") {
            return None;
        }
        let request = v.get("request")?;
        if request.get("subtype").and_then(|s| s.as_str()) != Some("can_use_tool") {
            return None;
        }
        Some(PermissionRequest {
            request_id: v.get("request_id").and_then(|r| r.as_str())?.to_string(),
            tool_name: request.get("tool_name").and_then(|t| t.as_str())?.to_string(),
            input: request.get("input").cloned().unwrap_or(serde_json::Value::Null),
        })
    }

    fn encode_permission_response(
        &self,
        request_id: &str,
        decision: &PermissionDecision,
    ) -> String {
        // The CLI rejects an `allow` that omits `updatedInput` (ZodError), so it
        // always carries the (verbatim) input back; a `deny` carries a message,
        // which for AskUserQuestion is read by the model as the operator's answer.
        let inner = match decision {
            PermissionDecision::Allow { updated_input } => serde_json::json!({
                "behavior": "allow",
                "updatedInput": updated_input,
            }),
            PermissionDecision::Deny { message } => serde_json::json!({
                "behavior": "deny",
                "message": message,
            }),
        };
        serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": inner,
            }
        })
        .to_string()
    }

    fn parse_result(&self, stdout: &str) -> Result<ClaudeOutput> {
        for line in stdout.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("type").and_then(|t| t.as_str()) == Some("result") {
                return serde_json::from_value::<ClaudeOutput>(v)
                    .context("parsing result event");
            }
        }
        anyhow::bail!("no result event found in stream-json output")
    }

    fn extract_session_id(&self, line: &str) -> Option<String> {
        if line.is_empty() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        v.get("session_id").and_then(|s| s.as_str()).map(String::from)
    }

    fn extract_output_tokens(&self, line: &str) -> Option<u64> {
        if line.is_empty() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let usage = v
            .get("usage")
            .or_else(|| v.get("message").and_then(|m| m.get("usage")))?;
        usage.get("output_tokens").and_then(|n| n.as_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_without_resume_is_interactive_stream_json() {
        let args = ClaudeCode.build_args(None);
        assert_eq!(
            args,
            vec![
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--replay-user-messages",
                "--permission-mode",
                "default",
                "--permission-prompt-tool",
                "stdio",
            ]
        );
    }

    #[test]
    fn build_args_with_resume_inserts_flag_after_print() {
        let args = ClaudeCode.build_args(Some("sess-123"));
        assert_eq!(&args[..3], &["--print", "-r", "sess-123"]);
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--input-format".to_string()));
        // The control-protocol flags coexist with --resume.
        assert!(args.contains(&"--permission-prompt-tool".to_string()));
        assert!(args.contains(&"stdio".to_string()));
    }

    #[test]
    fn encode_user_message_is_one_stream_json_line() {
        let line = ClaudeCode.encode_user_message("hello there");
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "text");
        assert_eq!(v["message"]["content"][0]["text"], "hello there");
    }

    #[test]
    fn parse_permission_request_reads_can_use_tool() {
        let line = r#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls -la"}}}"#;
        let req = ClaudeCode.parse_permission_request(line).expect("some");
        assert_eq!(req.request_id, "req-1");
        assert_eq!(req.tool_name, "Bash");
        assert_eq!(req.input["command"], "ls -la");
    }

    #[test]
    fn parse_permission_request_ignores_normal_events() {
        assert!(ClaudeCode
            .parse_permission_request(r#"{"type":"assistant","message":{}}"#)
            .is_none());
        assert!(ClaudeCode.parse_permission_request("not json").is_none());
    }

    #[test]
    fn parse_permission_request_ignores_other_control_requests() {
        let line = r#"{"type":"control_request","request_id":"req-2","request":{"subtype":"interrupt"}}"#;
        assert!(ClaudeCode.parse_permission_request(line).is_none());
    }

    #[test]
    fn encode_permission_response_allow_carries_updated_input() {
        let input = serde_json::json!({"command": "ls"});
        let line = ClaudeCode.encode_permission_response(
            "req-9",
            &PermissionDecision::Allow { updated_input: input.clone() },
        );
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "control_response");
        assert_eq!(v["response"]["subtype"], "success");
        assert_eq!(v["response"]["request_id"], "req-9");
        assert_eq!(v["response"]["response"]["behavior"], "allow");
        assert_eq!(v["response"]["response"]["updatedInput"], input);
    }

    #[test]
    fn encode_permission_response_deny_carries_message() {
        let line = ClaudeCode.encode_permission_response(
            "req-10",
            &PermissionDecision::Deny { message: "nope".into() },
        );
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["response"]["request_id"], "req-10");
        assert_eq!(v["response"]["response"]["behavior"], "deny");
        assert_eq!(v["response"]["response"]["message"], "nope");
    }

    #[test]
    fn parse_result_picks_the_result_event() {
        let stdout = concat!(
            r#"{"type":"system","subtype":"init","session_id":"abc"}"#,
            "\n",
            r#"{"type":"assistant","message":{"usage":{"output_tokens":5}}}"#,
            "\n",
            r#"{"type":"result","result":"done","session_id":"abc","total_cost_usd":0.12,"is_error":false,"num_turns":3,"output_tokens":42}"#,
            "\n",
        );
        let out = ClaudeCode.parse_result(stdout).unwrap();
        assert_eq!(out.result, "done");
        assert_eq!(out.session_id, "abc");
        assert!(!out.is_error);
        assert_eq!(out.num_turns, 3);
        assert_eq!(out.output_tokens, 42);
    }

    #[test]
    fn parse_result_errors_without_result_event() {
        let stdout = r#"{"type":"assistant","message":{}}"#;
        assert!(ClaudeCode.parse_result(stdout).is_err());
    }

    #[test]
    fn extract_session_id_reads_top_level_field() {
        let line = r#"{"type":"system","session_id":"sess-9"}"#;
        assert_eq!(ClaudeCode.extract_session_id(line).as_deref(), Some("sess-9"));
        assert_eq!(ClaudeCode.extract_session_id(r#"{"type":"x"}"#), None);
        assert_eq!(ClaudeCode.extract_session_id(""), None);
    }

    #[test]
    fn extract_output_tokens_reads_top_level_and_nested() {
        let top = r#"{"usage":{"output_tokens":10}}"#;
        assert_eq!(ClaudeCode.extract_output_tokens(top), Some(10));
        let nested = r#"{"message":{"usage":{"output_tokens":7}}}"#;
        assert_eq!(ClaudeCode.extract_output_tokens(nested), Some(7));
        assert_eq!(ClaudeCode.extract_output_tokens(r#"{"type":"x"}"#), None);
        assert_eq!(ClaudeCode.extract_output_tokens(""), None);
    }
}
