use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::agent::{AgentBackend, WorktreeFile};
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

    fn extra_env(&self, task_id: Uuid, agent_port: &str) -> Vec<(String, String)> {
        // The PreToolUse authcheck hook reads these to call back into the agent.
        vec![
            ("CLAUDE_TASK_ID".to_string(), task_id.to_string()),
            ("AGENT_PORT".to_string(), agent_port.to_string()),
        ]
    }

    fn worktree_files(&self, authcheck_hook: &Path) -> Vec<WorktreeFile> {
        let hook = authcheck_hook.to_string_lossy();
        // `bypassPermissions` skips Claude Code's interactive permission prompts
        // (which can't be answered in headless `-p` mode). The Bash +
        // AskUserQuestion PreToolUse hooks still fire — they're the actual
        // policy layer — so this only unblocks Edit/Write/Read/etc.
        //
        // `settings.local.json` is the conventional per-machine override file
        // (typically gitignored), so it doesn't need to be committed by the
        // project.
        let body = serde_json::json!({
            "permissions": { "defaultMode": "bypassPermissions" },
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "hooks": [{ "type": "command", "command": hook }] },
                    { "matcher": "AskUserQuestion", "hooks": [{ "type": "command", "command": hook }] }
                ]
            }
        });
        vec![WorktreeFile {
            rel_path: PathBuf::from(".claude/settings.local.json"),
            contents: serde_json::to_string_pretty(&body).unwrap_or_default(),
        }]
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
    use std::path::Path;

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
            ]
        );
    }

    #[test]
    fn build_args_with_resume_inserts_flag_after_print() {
        let args = ClaudeCode.build_args(Some("sess-123"));
        assert_eq!(&args[..3], &["--print", "-r", "sess-123"]);
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--input-format".to_string()));
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

    #[test]
    fn worktree_files_emits_settings_with_hook_and_bypass() {
        let files = ClaudeCode.worktree_files(Path::new("/hooks/authcheck.sh"));
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.rel_path, Path::new(".claude/settings.local.json"));
        assert!(f.contents.contains("bypassPermissions"));
        assert!(f.contents.contains("/hooks/authcheck.sh"));
        assert!(f.contents.contains("AskUserQuestion"));
    }
}
