<script setup lang="ts">
// Render the stream as turns: each turn shows its Input (the operator/trigger
// prompt) and Output (the result summary, or a streaming indicator while the
// turn is in flight). The raw events behind a turn are tucked behind an expand
// affordance that reuses ClaudeStream verbatim — same newline-delimited JSON it
// already consumes. Newest turn first; events natural order inside the expansion.

import { computed } from "vue";
import type { AuthRequest } from "../types/api";
import type { Turn } from "../composables/useTurns";
import ClaudeStream from "./ClaudeStream.vue";
import MarkdownView from "./MarkdownView.vue";

const props = defineProps<{
  turns: Turn[];
  /** Pending auth_requests for this task. They belong to the in-flight turn. */
  pending?: AuthRequest[];
}>();
const emit = defineEmits<{ resolved: [AuthRequest] }>();

// Newest turn at the top — the operator sees fresh activity without scrolling.
const ordered = computed(() => [...props.turns].reverse());

// ClaudeStream consumes newline-delimited JSON; serialize the turn's raw events.
function eventsText(turn: Turn): string {
  return turn.events.map((e) => JSON.stringify(e)).join("\n");
}

// Pending approvals match unresolved tool_uses, which only live in the streaming
// turn — hand them to the open turn, none to closed ones.
function pendingFor(turn: Turn): AuthRequest[] {
  return turn.open ? (props.pending ?? []) : [];
}
</script>

<template>
  <div v-if="turns.length === 0" class="text-sm text-faint">No events yet.</div>
  <div v-else class="space-y-3">
    <article
      v-for="turn in ordered"
      :key="turn.index"
      class="rounded-md border border-line bg-panel-2/40"
    >
      <header class="flex items-baseline gap-2 border-b border-line px-3 py-2">
        <span class="text-[10px] uppercase tracking-label text-faint">turn {{ turn.index + 1 }}</span>
        <span v-if="turn.open" class="inline-flex items-center gap-1 text-[11px] text-signal-live">
          <span class="led led-pulse text-signal-live" /> streaming…
        </span>
      </header>

      <div class="space-y-3 px-3 py-3">
        <!-- Input: the operator/trigger prompt that opened the turn. -->
        <div v-if="turn.inputText">
          <div class="mb-1 text-[10px] uppercase tracking-label text-faint">input</div>
          <div class="rounded border border-line bg-panel/60 px-3 py-2">
            <MarkdownView :source="turn.inputText" />
          </div>
        </div>

        <!-- Output: the result summary, or a placeholder while still streaming. -->
        <div>
          <div class="mb-1 text-[10px] uppercase tracking-label text-faint">output</div>
          <div
            v-if="turn.result"
            class="rounded border p-3"
            :class="turn.result.isError ? 'border-signal-danger/40 bg-signal-danger/5' : 'border-line bg-panel/60'"
          >
            <dl class="mb-2 grid grid-cols-3 gap-3 text-xs">
              <div v-if="turn.result.costUsd != null">
                <dt class="label mb-0.5">Cost</dt>
                <dd class="font-mono text-muted">${{ turn.result.costUsd.toFixed(4) }}</dd>
              </div>
              <div v-if="turn.result.numTurns != null">
                <dt class="label mb-0.5">Steps</dt>
                <dd class="font-mono text-muted">{{ turn.result.numTurns }}</dd>
              </div>
              <div v-if="turn.result.inputTokens != null || turn.result.outputTokens != null">
                <dt class="label mb-0.5">Tokens in / out</dt>
                <dd class="font-mono text-muted">{{ turn.result.inputTokens ?? "?" }} / {{ turn.result.outputTokens ?? "?" }}</dd>
              </div>
            </dl>
            <pre
              v-if="turn.result.isError"
              class="whitespace-pre-wrap font-mono text-xs text-signal-danger"
            >{{ turn.result.resultText }}</pre>
            <MarkdownView v-else-if="turn.result.resultText" :source="turn.result.resultText" />
            <p v-else class="text-xs text-muted">No result text.</p>
          </div>
          <p v-else class="text-xs text-muted">
            {{ turn.open ? "Working…" : "No result for this turn." }}
          </p>
        </div>

        <!-- Expand: this turn's raw events via the existing ClaudeStream. -->
        <details>
          <summary class="cursor-pointer text-[11px] text-faint hover:text-ink">
            {{ turn.events.length }} event{{ turn.events.length === 1 ? "" : "s" }}
          </summary>
          <div class="mt-2">
            <ClaudeStream
              :text="eventsText(turn)"
              :pending="pendingFor(turn)"
              @resolved="(r) => emit('resolved', r)"
            />
          </div>
        </details>
      </div>
    </article>
  </div>
</template>
