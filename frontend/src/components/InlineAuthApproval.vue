<script setup lang="ts">
// Lists pending auth_requests for a specific task and lets the operator
// approve/deny inline, so they don't have to switch to the Auth queue page
// while watching a task run.

import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { authApi } from "../api/auth";
import type { AuthRequest } from "../types/api";

const props = defineProps<{ taskId: string; poll: boolean }>();

const items = ref<AuthRequest[]>([]);
const replies = ref<Record<string, string>>({});
const busy = ref<Record<string, "approve" | "deny" | null>>({});
const error = ref<string | null>(null);

let timer: ReturnType<typeof setInterval> | null = null;

async function refresh() {
  try {
    items.value = await authApi.list({ task_id: props.taskId, status: "pending" });
    error.value = null;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  }
}

function startPolling() {
  stopPolling();
  if (!props.poll) return;
  timer = setInterval(refresh, 2000);
}
function stopPolling() {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

onMounted(async () => {
  await refresh();
  startPolling();
});
watch(() => props.poll, (p) => (p ? startPolling() : stopPolling()));
watch(() => props.taskId, refresh);
onUnmounted(stopPolling);

const heading = computed(() =>
  items.value.length === 1
    ? "1 pending approval"
    : `${items.value.length} pending approvals`,
);

async function resolve(item: AuthRequest, decision: "approve" | "deny") {
  busy.value[item.id] = decision;
  try {
    await authApi.resolve(item.id, decision, replies.value[item.id] || undefined);
    // Optimistic: remove from list — server will also notify the hook.
    items.value = items.value.filter((i) => i.id !== item.id);
    delete replies.value[item.id];
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value[item.id] = null;
  }
}

// Detect the AskUserQuestion prompt (formatted by the auth handler) so we can
// hint the operator that the reply IS the answer.
function isQuestion(prompt: string): boolean {
  return prompt.startsWith("Claude is asking the operator a question");
}
</script>

<template>
  <section v-if="items.length > 0" class="space-y-3 rounded-lg border border-accent/50 bg-accent/5 p-4">
    <header class="flex items-center gap-2">
      <span class="led led-pulse text-accent" />
      <span class="font-medium text-accent">{{ heading }}</span>
    </header>
    <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>
    <article
      v-for="item in items"
      :key="item.id"
      class="space-y-2 rounded-md border border-line bg-panel p-3"
    >
      <pre class="whitespace-pre-wrap font-sans text-sm leading-snug text-ink">{{ item.prompt_to_operator }}</pre>
      <div>
        <label class="label">
          {{ isQuestion(item.prompt_to_operator) ? "Your answer" : "Optional reply" }}
        </label>
        <textarea
          v-model="replies[item.id]"
          rows="2"
          class="textarea font-mono"
          :placeholder="
            isQuestion(item.prompt_to_operator)
              ? 'Required — this text is fed back to claude as the tool result.'
              : 'Optional note attached to your decision.'
          "
        />
      </div>
      <div class="flex items-center gap-2">
        <button :disabled="!!busy[item.id]" class="btn btn-primary btn-sm" @click="resolve(item, 'approve')">
          {{ busy[item.id] === "approve" ? "Approving…" : "Approve" }}
        </button>
        <button :disabled="!!busy[item.id]" class="btn btn-danger btn-sm" @click="resolve(item, 'deny')">
          {{ busy[item.id] === "deny" ? "Denying…" : "Deny" }}
        </button>
        <span class="ml-auto self-center font-mono text-[11px] text-faint">
          {{ new Date(item.created_at).toLocaleString() }}
        </span>
      </div>
    </article>
  </section>
</template>
