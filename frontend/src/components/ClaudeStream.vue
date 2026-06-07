<script setup lang="ts">
// Render claude's stream-json stdout as a readable timeline table. Parsing and
// approval pairing live in the useClaudeStream composable; this SFC is just
// presentation. Each row is a kind/role badge cell + a detail cell.

import { toRef } from "vue";
import type { AuthRequest } from "../types/api";
import AuthApprovalForm from "./AuthApprovalForm.vue";
import MarkdownView from "./MarkdownView.vue";
import {
  useClaudeStream,
  bashCommand,
  clamp,
  questionList,
  toolInputSummary,
} from "../composables/useClaudeStream";

const props = defineProps<{
  text: string;
  /** Pending auth_requests for the same task, oldest first. */
  pending?: AuthRequest[];
}>();
const emit = defineEmits<{ resolved: [AuthRequest] }>();

const { blocks } = useClaudeStream(toRef(props, "text"), toRef(props, "pending"));
</script>

<template>
  <div v-if="blocks.length === 0" class="text-sm text-faint">No events yet.</div>
  <div v-else class="overflow-x-auto">
    <table class="tbl text-xs">
      <tbody>
        <tr
          v-for="(b, i) in blocks"
          :key="i"
          :class="{
            'bg-panel/60': b.kind === 'init' || b.kind === 'unknown' || b.kind === 'system_note' || b.kind === 'rate_limit',
            'bg-panel': b.kind === 'text',
            'bg-accent/5': b.kind === 'tool_use' && b.awaitingApproval,
            'bg-signal-live/5': b.kind === 'tool_use' && !b.awaitingApproval,
            'bg-signal-ok/5': b.kind === 'tool_result' && !b.isError,
            'bg-signal-danger/5': (b.kind === 'tool_result' && b.isError) || b.kind === 'error',
          }"
        >
          <!-- Kind / role badge cell. -->
          <td class="w-px whitespace-nowrap align-top">
            <template v-if="b.kind === 'init'">
              <span class="font-medium text-muted">session start</span>
            </template>
            <template v-else-if="b.kind === 'text'">
              <span
                class="text-[10px] uppercase tracking-label"
                :class="b.role === 'assistant' ? 'text-accent' : 'text-faint'"
                >{{ b.role }}</span
              >
            </template>
            <template v-else-if="b.kind === 'tool_use'">
              <span :class="b.awaitingApproval ? 'text-accent' : 'text-signal-live'" class="font-medium">
                {{ b.awaitingApproval ? "⏸ awaiting" : "→" }} {{ b.name }}
              </span>
            </template>
            <template v-else-if="b.kind === 'tool_result'">
              <span :class="b.isError ? 'text-signal-danger' : 'text-signal-ok'" class="font-medium">
                {{ b.isError ? "✗ tool error" : "← tool result" }}
              </span>
            </template>
            <template v-else-if="b.kind === 'rate_limit'">
              <span class="font-medium text-muted">rate limit</span>
            </template>
            <template v-else-if="b.kind === 'system_note'">
              <span class="font-medium text-muted">{{ b.subtype }}</span>
            </template>
            <template v-else-if="b.kind === 'error'">
              <span class="text-[10px] uppercase tracking-label text-signal-danger">error</span>
            </template>
            <template v-else>
              <span class="font-medium text-muted">{{ b.type }}</span>
            </template>
          </td>

          <!-- Detail cell. -->
          <td class="align-top">
            <template v-if="b.kind === 'init'">
              <div class="font-mono text-faint">
                <span v-if="b.sessionId">{{ b.sessionId }}</span>
                <span v-if="b.cwd" class="ml-2">cwd: {{ b.cwd }}</span>
                <span v-if="b.toolCount != null" class="ml-2">tools: {{ b.toolCount }}</span>
              </div>
            </template>

            <template v-else-if="b.kind === 'text'">
              <MarkdownView :source="b.body" />
            </template>

            <template v-else-if="b.kind === 'tool_use'">
              <div class="space-y-2">
                <details>
                  <summary class="flex cursor-pointer items-baseline gap-2">
                    <span class="truncate font-mono text-[11px] text-muted">{{
                      clamp(toolInputSummary(b.input), 200)
                    }}</span>
                  </summary>
                  <pre class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-muted">{{ JSON.stringify(b.input, null, 2) }}</pre>
                </details>

                <!-- Shell command behind a Bash call, highlighted terminal-style. -->
                <pre
                  v-if="bashCommand(b)"
                  class="overflow-x-auto whitespace-pre-wrap font-mono text-[11px] leading-snug text-signal-live"
                ><span class="select-none text-signal-live/50">$ </span>{{ bashCommand(b) }}</pre>

                <!-- AskUserQuestion: each question + its options. -->
                <div v-if="questionList(b.input) && !b.awaitingApproval" class="space-y-2">
                  <div
                    v-for="(q, qi) in questionList(b.input)"
                    :key="qi"
                    class="border-l-2 border-signal-auth/60 pl-3"
                  >
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
                <summary class="flex cursor-pointer items-baseline gap-2">
                  <span class="truncate font-mono text-[11px] text-muted">{{
                    clamp(b.body.replace(/\s+/g, " "), 200)
                  }}</span>
                </summary>
                <pre class="max-h-64 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-muted">{{ b.body }}</pre>
              </details>
            </template>

            <template v-else-if="b.kind === 'rate_limit'">
              <div class="text-muted">
                <span>{{ b.rateLimitType ?? "?" }}</span>
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
              <span v-if="b.summary" class="text-faint">{{ b.summary }}</span>
            </template>

            <template v-else-if="b.kind === 'error'">
              <pre class="whitespace-pre-wrap font-mono text-xs text-signal-danger">{{ b.message }}</pre>
            </template>

            <template v-else>
              <details>
                <summary class="flex cursor-pointer items-baseline gap-2 text-muted">
                  <span v-if="b.summary" class="text-faint">{{ b.summary }}</span>
                </summary>
                <pre class="whitespace-pre-wrap font-mono text-[11px] text-faint">{{ b.raw }}</pre>
              </details>
            </template>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
