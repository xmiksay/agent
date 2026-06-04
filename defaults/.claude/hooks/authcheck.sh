#!/usr/bin/env bash
# PreToolUse hook for the agent. Handles three cases:
#
#  1. tool_name == "Bash": send the command to /internal/authcheck and let the
#     operator allow/deny (or match against the project allowlist).
#  2. tool_name == "AskUserQuestion": serialize the question + options, send to
#     the agent so the operator can answer it via the Auth queue, then block
#     the tool call and feed the operator's reply back to Claude as stderr.
#  3. Anything else: allow (exit 0).
#
# Environment:
#   CLAUDE_TASK_ID  — UUID of the supervising task.
#   AGENT_PORT      — TCP port of the agent (default: 3000).
set -euo pipefail

PORT="${AGENT_PORT:-3000}"
URL="http://127.0.0.1:${PORT}/internal/authcheck"

if [[ -z "${CLAUDE_TASK_ID:-}" ]]; then
    # Not running under the agent; allow.
    exit 0
fi

payload="$(cat)"

tool_name="$(printf '%s' "$payload" | jq -r '.tool_name // .tool // empty')"
questions_json=""

case "$tool_name" in
    Bash)
        command="$(printf '%s' "$payload" | jq -r '.tool_input.command // .toolInput.command // empty')"
        ;;
    AskUserQuestion)
        # Render each question as "Q: ...\n  - option (description)\n..." for a
        # readable fallback in the auth list, and also forward the raw questions
        # JSON so the frontend can render buttons.
        command="$(printf '%s' "$payload" | jq -r '
            (.tool_input.questions // .toolInput.questions // []) | map(
                "Q: " + .question +
                (if .multiSelect then " (multi-select)" else "" end) +
                "\n" +
                ((.options // []) | map("  - " + .label + (if .description then " — " + .description else "" end)) | join("\n"))
            ) | join("\n\n")')"
        if [[ -z "$command" ]]; then
            command="(empty AskUserQuestion payload)"
        fi
        questions_json="$(printf '%s' "$payload" | jq -c '.tool_input.questions // .toolInput.questions // []')"
        ;;
    *)
        exit 0
        ;;
esac

if [[ -z "$command" ]]; then
    exit 0
fi

if [[ -n "$questions_json" ]]; then
    request="$(jq -n --arg id "$CLAUDE_TASK_ID" --arg cmd "$command" --arg tool "$tool_name" --argjson questions "$questions_json" \
        '{task_id: $id, command: $cmd, tool: $tool, questions: $questions}')"
else
    request="$(jq -n --arg id "$CLAUDE_TASK_ID" --arg cmd "$command" --arg tool "$tool_name" \
        '{task_id: $id, command: $cmd, tool: $tool}')"
fi

response="$(curl --silent --show-error --max-time 620 \
    -H 'Content-Type: application/json' \
    --data "$request" \
    "$URL")" || {
    echo "authcheck hook: curl failed contacting $URL" >&2
    exit 1
}

allowed="$(printf '%s' "$response" | jq -r '.allowed')"
reply="$(printf '%s' "$response" | jq -r '.reply // empty')"
reason="$(printf '%s' "$response" | jq -r '.reason // empty')"

if [[ "$tool_name" == "AskUserQuestion" ]]; then
    # Block the interactive tool (there is no human at Claude's keyboard) and
    # feed the operator's reply back through stderr so Claude treats it as the
    # answer.
    if [[ "$allowed" == "true" ]]; then
        printf 'Operator answered the question:\n%s\n' "${reply:-(no reply text)}" >&2
    else
        printf 'Operator declined the question (%s). Continue using your best judgment.\n' "${reason:-denied}" >&2
    fi
    exit 2
fi

if [[ "$allowed" == "true" ]]; then
    # Tell Claude Code to SKIP its own permissions.allow check — the authcheck
    # is authoritative for Bash. Without `permissionDecision: "allow"`, Claude
    # Code falls back to settings.json and denies anything not on its allow
    # list with "This command requires approval".
    jq -n --arg reason "${reason:-authcheck approved}" --arg reply "$reply" '{
        hookSpecificOutput: ({
            hookEventName: "PreToolUse",
            permissionDecision: "allow",
            permissionDecisionReason: $reason
        } + (if $reply != "" then { additionalContext: ("Operator reply: " + $reply) } else {} end))
    }'
    exit 0
else
    msg="${reason:-denied}"
    if [[ -n "$reply" ]]; then
        msg="${msg} (reply: ${reply})"
    fi
    jq -n --arg msg "$msg" '{
        hookSpecificOutput: {
            hookEventName: "PreToolUse",
            permissionDecision: "deny",
            permissionDecisionReason: $msg
        }
    }'
    exit 0
fi
