<script setup lang="ts">
// Render claude's stream-json stdout as a readable timeline.
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
// auth_request for this task, we render an inline approve/deny widget on the
// tool_use card so the operator can decide right where the call happens.

import { computed } from "vue";
import type { AuthQuestion, AuthRequest } from "../types/api";
import AuthApprovalForm from "./AuthApprovalForm.vue";
import MarkdownView from "./MarkdownView.vue";

const props = defineProps<{
  text: string;
  /** Pending auth_requests for the same task, oldest first. */
  pending?: AuthRequest[];
}>();
const emit = defineEmits<{ resolved: [AuthRequest] }>();

interface InitBlock { kind: "init"; cwd?: string; sessionId?: string; toolCount?: number }
interface TextBlock { kind: "text"; role: "assistant" | "user"; body: string }
interface ToolUseBlock { kind: "tool_use"; name: string; input: unknown; id: string; awaitingApproval: AuthRequest | null }
interface ToolResultBlock { kind: "tool_result"; id: string; body: string; isError: boolean }
interface RateLimitBlock {
  kind: "rate_limit";
  status: string;
  rateLimitType?: string;
  resetsAt?: number;
  overageStatus?: string;
  overageReason?: string;
  isUsingOverage?: boolean;
}
interface SystemNoteBlock { kind: "system_note"; subtype: string; summary: string }
interface ErrorBlock { kind: "error"; message: string }
interface UnknownBlock { kind: "unknown"; type: string; summary: string; raw: string }
type Block =
  | InitBlock
  | TextBlock
  | ToolUseBlock
  | ToolResultBlock
  | RateLimitBlock
  | SystemNoteBlock
  | ErrorBlock
  | UnknownBlock;

const parsed = computed(() => {
  const blocks: Block[] = [];
  for (const rawLine of (props.text ?? "").split("\n")) {
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
      blocks.push({
        kind: "system_note",
        subtype: v.subtype ?? "system",
        summary: stringifyShort(v),
      });
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
});

function stringifyShort(v: any): string {
  // Best-effort one-line summary: prefer subtype, then a small key sample.
  if (typeof v.subtype === "string") return v.subtype;
  const keys = Object.keys(v).filter((k) => k !== "type" && k !== "uuid" && k !== "session_id");
  return keys.slice(0, 3).join(", ") || "";
}

const blocks = computed<Block[]>(() => {
  // Mark which tool_uses don't have a matching tool_result yet (still in flight).
  const resultIds = new Set(
    parsed.value
      .filter((b): b is ToolResultBlock => b.kind === "tool_result")
      .map((b) => b.id),
  );
  // Pair unresolved tool_uses with pending approvals, oldest first. The
  // hook serialises Bash command strings as the auth_request.requested_op,
  // and AskUserQuestion as a multi-line prompt; matching by ordering is
  // robust against both.
  const queue = [...(props.pending ?? [])].sort(
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
  // Any remaining approvals (no matching tool_use yet — race condition while
  // claude is mid-event) show as standalone amber cards at the end.
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

function stringifyToolBody(content: unknown): string {
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

function toolInputSummary(input: unknown): string {
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

function clamp(s: string, n: number): string {
  return s.length <= n ? s : `${s.slice(0, n)}…`;
}

// The shell command behind a Bash tool_use, so we can highlight it terminal-style.
function bashCommand(b: ToolUseBlock): string | null {
  if (b.name !== "Bash") return null;
  const o = b.input as Record<string, unknown> | null;
  return o && typeof o.command === "string" ? o.command : null;
}

// The structured questions behind an AskUserQuestion tool_use, if any.
function questionList(input: unknown): AuthQuestion[] | null {
  const q = (input as Record<string, unknown> | null)?.questions;
  return Array.isArray(q) && q.length > 0 ? (q as AuthQuestion[]) : null;
}
</script>

<template>
  <div v-if="blocks.length === 0" class="text-sm text-faint">No events yet.</div>
  <ol v-else class="space-y-2">
    <li
      v-for="(b, i) in blocks"
      :key="i"
      class="rounded-md border text-xs"
      :class="{
        'border-line bg-panel/60': b.kind === 'init' || b.kind === 'unknown' || b.kind === 'system_note' || b.kind === 'rate_limit',
        'border-line bg-panel': b.kind === 'text',
        'border-accent/60 bg-accent/5': b.kind === 'tool_use' && b.awaitingApproval,
        'border-signal-live/30 bg-signal-live/5': b.kind === 'tool_use' && !b.awaitingApproval,
        'border-signal-ok/30 bg-signal-ok/5': b.kind === 'tool_result' && !b.isError,
        'border-signal-danger/40 bg-signal-danger/5': (b.kind === 'tool_result' && b.isError) || b.kind === 'error',
      }"
    >
      <template v-if="b.kind === 'init'">
        <div class="px-3 py-2 font-mono text-faint">
          <span class="font-medium text-muted">session start</span>
          <span v-if="b.sessionId" class="ml-2">{{ b.sessionId }}</span>
          <span v-if="b.cwd" class="ml-2">cwd: {{ b.cwd }}</span>
          <span v-if="b.toolCount != null" class="ml-2">tools: {{ b.toolCount }}</span>
        </div>
      </template>

      <template v-else-if="b.kind === 'text'">
        <div class="px-3 py-2">
          <div class="mb-1 text-[10px] uppercase tracking-label" :class="b.role === 'assistant' ? 'text-accent' : 'text-faint'">{{ b.role }}</div>
          <MarkdownView :source="b.body" />
        </div>
      </template>

      <template v-else-if="b.kind === 'tool_use'">
        <div class="space-y-2 px-3 py-2">
          <details>
            <summary class="flex cursor-pointer items-baseline gap-2">
              <span :class="b.awaitingApproval ? 'text-accent' : 'text-signal-live'" class="font-medium">
                {{ b.awaitingApproval ? "⏸ awaiting approval" : "→" }} {{ b.name }}
              </span>
              <span class="truncate font-mono text-[11px] text-muted">{{ clamp(toolInputSummary(b.input), 200) }}</span>
            </summary>
            <pre class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-muted">{{ JSON.stringify(b.input, null, 2) }}</pre>
          </details>

          <!-- Highlight the shell command behind a Bash call, terminal-style. -->
          <div v-if="bashCommand(b)" class="overflow-hidden rounded border border-signal-live/30">
            <div class="bg-signal-live/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-label text-signal-live">bash</div>
            <pre class="overflow-x-auto bg-canvas px-3 py-2 font-mono text-[11px] leading-snug text-signal-live">{{ bashCommand(b) }}</pre>
          </div>

          <!-- AskUserQuestion: show each question + its options (interactive form
               below when this call is awaiting approval). -->
          <div v-if="questionList(b.input) && !b.awaitingApproval" class="space-y-2">
            <div v-for="(q, qi) in questionList(b.input)" :key="qi" class="border-l-2 border-signal-auth/60 pl-3">
              <div class="text-xs font-medium text-ink">
                {{ q.question }}
                <span v-if="q.multiSelect" class="ml-1 text-[10px] uppercase tracking-label text-faint">multi</span>
              </div>
              <ul class="mt-1 space-y-0.5">
                <li v-for="opt in q.options" :key="opt.label" class="text-[11px] text-muted">
                  <span class="font-medium text-ink">{{ opt.label }}</span>
                  <span v-if="opt.description"> — {{ opt.description }}</span>
                </li>
              </ul>
            </div>
          </div>

          <div v-if="b.awaitingApproval" class="border-t border-accent/30 pt-2">
            <pre class="mb-2 whitespace-pre-wrap font-sans text-xs leading-snug text-accent">{{ b.awaitingApproval.prompt_to_operator }}</pre>
            <AuthApprovalForm :item="b.awaitingApproval" compact @resolved="(r) => emit('resolved', r)" />
          </div>
        </div>
      </template>

      <template v-else-if="b.kind === 'tool_result'">
        <details>
          <summary class="flex cursor-pointer items-baseline gap-2 px-3 py-2">
            <span :class="b.isError ? 'text-signal-danger' : 'text-signal-ok'" class="font-medium">
              {{ b.isError ? "✗ tool error" : "← tool result" }}
            </span>
            <span class="truncate font-mono text-[11px] text-muted">{{ clamp(b.body.replace(/\s+/g, " "), 200) }}</span>
          </summary>
          <pre class="max-h-64 overflow-auto whitespace-pre-wrap px-3 pb-2 font-mono text-[11px] text-muted">{{ b.body }}</pre>
        </details>
      </template>

      <template v-else-if="b.kind === 'rate_limit'">
        <div class="px-3 py-2 text-muted">
          <span class="font-medium">rate limit</span>
          <span class="ml-2">{{ b.rateLimitType ?? "?" }}</span>
          <span
            class="ml-2 rounded px-1.5 py-0.5 text-[10px] uppercase tracking-label"
            :class="b.status === 'allowed' ? 'bg-signal-ok/15 text-signal-ok' : 'bg-signal-danger/15 text-signal-danger'"
          >
            {{ b.status }}
          </span>
          <span v-if="b.resetsAt" class="ml-2 text-faint">
            resets {{ new Date(b.resetsAt * 1000).toLocaleString() }}
          </span>
          <span v-if="b.overageStatus" class="ml-2 text-faint">
            overage: {{ b.overageStatus
              }}{{ b.overageReason ? ` (${b.overageReason})` : "" }}{{ b.isUsingOverage ? ", active" : "" }}
          </span>
        </div>
      </template>

      <template v-else-if="b.kind === 'system_note'">
        <div class="px-3 py-2 text-muted">
          <span class="font-medium">{{ b.subtype }}</span>
          <span v-if="b.summary" class="ml-2 text-faint">{{ b.summary }}</span>
        </div>
      </template>

      <template v-else-if="b.kind === 'error'">
        <div class="px-3 py-2">
          <div class="mb-1 text-[10px] uppercase tracking-label text-signal-danger">error</div>
          <pre class="whitespace-pre-wrap font-mono text-xs text-signal-danger">{{ b.message }}</pre>
        </div>
      </template>

      <template v-else>
        <details>
          <summary class="flex cursor-pointer items-baseline gap-2 px-3 py-2 text-muted">
            <span class="font-medium">{{ b.type }}</span>
            <span v-if="b.summary" class="text-faint">{{ b.summary }}</span>
          </summary>
          <pre class="whitespace-pre-wrap px-3 pb-2 font-mono text-[11px] text-faint">{{ b.raw }}</pre>
        </details>
      </template>
    </li>
  </ol>
</template>
