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
import type { AuthRequest } from "../types/api";
import AuthApprovalForm from "./AuthApprovalForm.vue";

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
</script>

<template>
  <div v-if="blocks.length === 0" class="text-sm text-gray-500">
    No events yet.
  </div>
  <ol v-else class="space-y-2">
    <li
      v-for="(b, i) in blocks"
      :key="i"
      class="rounded border text-xs"
      :class="{
        'border-gray-200 bg-gray-50': b.kind === 'init' || b.kind === 'unknown' || b.kind === 'system_note' || b.kind === 'rate_limit',
        'border-gray-200 bg-white': b.kind === 'text',
        'border-amber-300 bg-amber-50': b.kind === 'tool_use' && b.awaitingApproval,
        'border-blue-200 bg-blue-50': b.kind === 'tool_use' && !b.awaitingApproval,
        'border-emerald-200 bg-emerald-50': b.kind === 'tool_result' && !b.isError,
        'border-red-200 bg-red-50': (b.kind === 'tool_result' && b.isError) || b.kind === 'error',
      }"
    >
      <template v-if="b.kind === 'init'">
        <div class="px-3 py-2 text-gray-600">
          <span class="font-medium">session start</span>
          <span v-if="b.sessionId" class="ml-2 font-mono">{{ b.sessionId }}</span>
          <span v-if="b.cwd" class="ml-2 text-gray-500">cwd: {{ b.cwd }}</span>
          <span v-if="b.toolCount != null" class="ml-2 text-gray-500">
            tools: {{ b.toolCount }}
          </span>
        </div>
      </template>

      <template v-else-if="b.kind === 'text'">
        <div class="px-3 py-2">
          <div class="text-[10px] uppercase tracking-wide text-gray-500 mb-1">
            {{ b.role }}
          </div>
          <pre class="whitespace-pre-wrap font-sans text-sm leading-snug">{{ b.body }}</pre>
        </div>
      </template>

      <template v-else-if="b.kind === 'tool_use'">
        <div class="px-3 py-2 space-y-2">
          <details>
            <summary class="cursor-pointer flex items-baseline gap-2">
              <span :class="b.awaitingApproval ? 'text-amber-800' : 'text-blue-700'" class="font-medium">
                {{ b.awaitingApproval ? "⏸ awaiting approval" : "→" }} {{ b.name }}
              </span>
              <span class="text-gray-700 font-mono text-[11px] truncate">
                {{ clamp(toolInputSummary(b.input), 200) }}
              </span>
            </summary>
            <pre class="mt-2 text-[11px] font-mono whitespace-pre-wrap overflow-auto max-h-64">{{ JSON.stringify(b.input, null, 2) }}</pre>
          </details>
          <div v-if="b.awaitingApproval" class="border-t border-amber-200 pt-2">
            <pre class="whitespace-pre-wrap font-sans text-xs leading-snug text-amber-900 mb-2">{{ b.awaitingApproval.prompt_to_operator }}</pre>
            <AuthApprovalForm
              :item="b.awaitingApproval"
              compact
              @resolved="(r) => emit('resolved', r)"
            />
          </div>
        </div>
      </template>

      <template v-else-if="b.kind === 'tool_result'">
        <details>
          <summary class="px-3 py-2 cursor-pointer flex items-baseline gap-2">
            <span :class="b.isError ? 'text-red-700' : 'text-emerald-700'" class="font-medium">
              {{ b.isError ? "✗ tool error" : "← tool result" }}
            </span>
            <span class="text-gray-700 font-mono text-[11px] truncate">
              {{ clamp(b.body.replace(/\s+/g, " "), 200) }}
            </span>
          </summary>
          <pre class="px-3 pb-2 text-[11px] font-mono whitespace-pre-wrap overflow-auto max-h-64">{{ b.body }}</pre>
        </details>
      </template>

      <template v-else-if="b.kind === 'rate_limit'">
        <div class="px-3 py-2 text-gray-700">
          <span class="font-medium">rate limit</span>
          <span class="ml-2">{{ b.rateLimitType ?? "?" }}</span>
          <span
            class="ml-2 px-1.5 py-0.5 rounded text-[10px] uppercase tracking-wide"
            :class="b.status === 'allowed' ? 'bg-emerald-100 text-emerald-800' : 'bg-red-100 text-red-800'"
          >
            {{ b.status }}
          </span>
          <span v-if="b.resetsAt" class="ml-2 text-gray-500">
            resets {{ new Date(b.resetsAt * 1000).toLocaleString() }}
          </span>
          <span v-if="b.overageStatus" class="ml-2 text-gray-500">
            overage: {{ b.overageStatus
              }}{{ b.overageReason ? ` (${b.overageReason})` : "" }}{{ b.isUsingOverage ? ", active" : "" }}
          </span>
        </div>
      </template>

      <template v-else-if="b.kind === 'system_note'">
        <div class="px-3 py-2 text-gray-600">
          <span class="font-medium">{{ b.subtype }}</span>
          <span v-if="b.summary" class="ml-2 text-gray-500">{{ b.summary }}</span>
        </div>
      </template>

      <template v-else-if="b.kind === 'error'">
        <div class="px-3 py-2">
          <div class="text-[10px] uppercase tracking-wide text-red-700 mb-1">error</div>
          <pre class="whitespace-pre-wrap font-mono text-xs">{{ b.message }}</pre>
        </div>
      </template>

      <template v-else>
        <details>
          <summary class="px-3 py-2 cursor-pointer flex items-baseline gap-2 text-gray-600">
            <span class="font-medium">{{ b.type }}</span>
            <span v-if="b.summary" class="text-gray-500">{{ b.summary }}</span>
          </summary>
          <pre class="px-3 pb-2 font-mono whitespace-pre-wrap text-[11px] text-gray-600">{{ b.raw }}</pre>
        </details>
      </template>
    </li>
  </ol>
</template>
