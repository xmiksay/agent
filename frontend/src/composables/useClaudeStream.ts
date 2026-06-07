// Parse claude's stream-json stdout into a readable timeline of typed blocks.
//
// Each line in `text` is a JSON event of one of these shapes:
//   { type:"system", subtype:"init", session_id, cwd, tools, ... }
//   { type:"assistant", message: { content: [
//         { type:"text", text } | { type:"tool_use", name, input, id }
//     ]}}
//   { type:"user", message: { content: [
//         { type:"tool_result", tool_use_id, content, is_error? }
//     ]}}
//   { type:"result", is_error, result, total_cost_usd, num_turns, ... }
//
// The final `result` event is shown elsewhere on the page; we skip it here.
//
// If a tool_use doesn't have a matching tool_result yet AND there's a pending
// auth_request for this task, the consumer renders an inline approve/deny widget
// on the tool_use row so the operator can decide right where the call happens.

import { computed, type Ref } from "vue";
import type { AuthQuestion, AuthRequest } from "../types/api";

export interface InitBlock { kind: "init"; cwd?: string; sessionId?: string; toolCount?: number }
export interface TextBlock { kind: "text"; role: "assistant" | "user"; body: string }
export interface ToolUseBlock { kind: "tool_use"; name: string; input: unknown; id: string; awaitingApproval: AuthRequest | null }
export interface ToolResultBlock { kind: "tool_result"; id: string; body: string; isError: boolean }
export interface RateLimitBlock {
  kind: "rate_limit";
  status: string;
  rateLimitType?: string;
  resetsAt?: number;
  overageStatus?: string;
  overageReason?: string;
  isUsingOverage?: boolean;
}
export interface SystemNoteBlock { kind: "system_note"; subtype: string; summary: string }
export interface ErrorBlock { kind: "error"; message: string }
export interface UnknownBlock { kind: "unknown"; type: string; summary: string; raw: string }
export type Block =
  | InitBlock
  | TextBlock
  | ToolUseBlock
  | ToolResultBlock
  | RateLimitBlock
  | SystemNoteBlock
  | ErrorBlock
  | UnknownBlock;

function stringifyShort(v: any): string {
  // Best-effort one-line summary: prefer subtype, then a small key sample.
  if (typeof v.subtype === "string") return v.subtype;
  const keys = Object.keys(v).filter((k) => k !== "type" && k !== "uuid" && k !== "session_id");
  return keys.slice(0, 3).join(", ") || "";
}

export function stringifyToolBody(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((c) => {
        if (typeof c === "string") return c;
        if (c && typeof c === "object" && "text" in c && typeof (c as any).text === "string") {
          return (c as { text: string }).text;
        }
        return JSON.stringify(c);
      })
      .join("\n");
  }
  if (content == null) return "";
  return JSON.stringify(content, null, 2);
}

export function toolInputSummary(input: unknown): string {
  if (input == null) return "";
  if (typeof input === "string") return input;
  if (typeof input !== "object") return String(input);
  const o = input as Record<string, unknown>;
  if (typeof o.command === "string") return o.command;
  if (typeof o.file_path === "string") {
    const action = typeof o.old_string === "string" ? "edit" : "read/write";
    return `${o.file_path} (${action})`;
  }
  if (typeof o.pattern === "string") return o.pattern;
  if (typeof o.path === "string") return o.path;
  if (typeof o.prompt === "string") return o.prompt;
  return JSON.stringify(o);
}

export function clamp(s: string, n: number): string {
  return s.length <= n ? s : `${s.slice(0, n)}…`;
}

// The shell command behind a Bash tool_use, so we can highlight it terminal-style.
export function bashCommand(b: ToolUseBlock): string | null {
  if (b.name !== "Bash") return null;
  const o = b.input as Record<string, unknown> | null;
  return o && typeof o.command === "string" ? o.command : null;
}

// The structured questions behind an AskUserQuestion tool_use, if any.
export function questionList(input: unknown): AuthQuestion[] | null {
  const q = (input as Record<string, unknown> | null)?.questions;
  return Array.isArray(q) && q.length > 0 ? (q as AuthQuestion[]) : null;
}

function parseLines(text: string): Block[] {
  const blocks: Block[] = [];
  for (const rawLine of (text ?? "").split("\n")) {
    const line = rawLine.trim();
    if (!line) continue;
    let v: any;
    try {
      v = JSON.parse(line);
    } catch {
      blocks.push({ kind: "unknown", type: "?", summary: "(unparseable line)", raw: line });
      continue;
    }
    if (!v || typeof v !== "object") continue;
    if (v.type === "result") continue;
    // Thinking-token accounting events are noise to the operator — drop them.
    if (v.type === "thinking_tokens" || v.subtype === "thinking_tokens") continue;
    if (v.type === "system" && v.subtype === "init") {
      blocks.push({
        kind: "init",
        cwd: v.cwd,
        sessionId: v.session_id,
        toolCount: Array.isArray(v.tools) ? v.tools.length : undefined,
      });
      continue;
    }
    if (v.type === "system") {
      blocks.push({ kind: "system_note", subtype: v.subtype ?? "system", summary: stringifyShort(v) });
      continue;
    }
    if (v.type === "rate_limit_event") {
      const info = v.rate_limit_info ?? {};
      blocks.push({
        kind: "rate_limit",
        status: info.status ?? "unknown",
        rateLimitType: info.rateLimitType,
        resetsAt: typeof info.resetsAt === "number" ? info.resetsAt : undefined,
        overageStatus: info.overageStatus,
        overageReason: info.overageDisabledReason,
        isUsingOverage: info.isUsingOverage,
      });
      continue;
    }
    if (v.type === "error") {
      blocks.push({
        kind: "error",
        message:
          typeof v.message === "string"
            ? v.message
            : typeof v.error === "string"
              ? v.error
              : JSON.stringify(v, null, 2),
      });
      continue;
    }
    if (v.type === "assistant" && v.message?.content) {
      for (const c of v.message.content) {
        if (c.type === "text" && typeof c.text === "string") {
          blocks.push({ kind: "text", role: "assistant", body: c.text });
        } else if (c.type === "tool_use") {
          blocks.push({
            kind: "tool_use",
            name: c.name ?? "tool",
            input: c.input,
            id: c.id ?? "",
            awaitingApproval: null,
          });
        }
      }
      continue;
    }
    if (v.type === "user" && v.message?.content) {
      for (const c of v.message.content) {
        if (c.type === "tool_result") {
          blocks.push({
            kind: "tool_result",
            id: c.tool_use_id ?? "",
            body: stringifyToolBody(c.content),
            isError: !!c.is_error,
          });
        } else if (c.type === "text" && typeof c.text === "string") {
          blocks.push({ kind: "text", role: "user", body: c.text });
        }
      }
      continue;
    }
    blocks.push({
      kind: "unknown",
      type: typeof v.type === "string" ? v.type : "?",
      summary: stringifyShort(v),
      raw: line,
    });
  }
  return blocks;
}

// claude replays user messages, so an identical user text block can land twice
// back-to-back. Collapse only exact consecutive duplicates of user messages.
function dedupeUserEchoes(blocks: Block[]): Block[] {
  const out: Block[] = [];
  for (const b of blocks) {
    const prev = out[out.length - 1];
    if (
      b.kind === "text" &&
      b.role === "user" &&
      prev?.kind === "text" &&
      prev.role === "user" &&
      prev.body.trim() === b.body.trim()
    ) {
      continue;
    }
    out.push(b);
  }
  return out;
}

/** Reactive timeline: parsed blocks paired with pending approvals, newest first. */
export function useClaudeStream(text: Ref<string>, pending: Ref<AuthRequest[] | undefined>) {
  const parsed = computed(() => dedupeUserEchoes(parseLines(text.value)));

  const blocks = computed<Block[]>(() => {
    // Mark which tool_uses don't have a matching tool_result yet (still in flight).
    const resultIds = new Set(
      parsed.value.filter((b): b is ToolResultBlock => b.kind === "tool_result").map((b) => b.id),
    );
    // Pair unresolved tool_uses with pending approvals, oldest first. The hook
    // serialises Bash command strings as the auth_request.requested_op, and
    // AskUserQuestion as a multi-line prompt; matching by ordering is robust
    // against both.
    const queue = [...(pending.value ?? [])].sort(
      (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime(),
    );
    const result: Block[] = [];
    for (const b of parsed.value) {
      if (b.kind === "tool_use" && !resultIds.has(b.id) && queue.length > 0) {
        const auth = queue.shift()!;
        result.push({ ...b, awaitingApproval: auth });
      } else {
        result.push(b);
      }
    }
    // Any remaining approvals (no matching tool_use yet — race while claude is
    // mid-event) show as standalone amber rows at the end.
    for (const auth of queue) {
      result.push({
        kind: "tool_use",
        name: "pending approval",
        input: { prompt: auth.prompt_to_operator },
        id: auth.id,
        awaitingApproval: auth,
      });
    }
    // Reverse so newest events are at the top — the operator sees fresh activity
    // without scrolling on long-running tasks.
    return result.reverse();
  });

  return { blocks };
}
