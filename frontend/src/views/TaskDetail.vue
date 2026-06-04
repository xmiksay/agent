<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import ClaudeStream from "../components/ClaudeStream.vue";
import TriggerView from "../components/TriggerView.vue";
import MarkdownView from "../components/MarkdownView.vue";
import { authApi } from "../api/auth";
import type { AuthRequest } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useTasksStore();
const router = useRouter();
const busy = ref<string | null>(null);
const showOutput = ref(false);
const pendingApprovals = ref<AuthRequest[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;

async function reloadPending() {
  try {
    pendingApprovals.value = await authApi.list({
      task_id: props.id,
      status: "pending",
    });
  } catch {
    /* ignore — banner will just be empty */
  }
}

async function reload() {
  await store.load(props.id);
  await Promise.all([
    showOutput.value ? store.loadOutput(props.id) : Promise.resolve(),
    reloadPending(),
  ]);
}

function onApprovalResolved(resolved: AuthRequest) {
  pendingApprovals.value = pendingApprovals.value.filter((p) => p.id !== resolved.id);
}

// Auto-expand the output section the moment an approval is pending — the
// operator's controls live inside the matching tool_use card.
watch(pendingApprovals, async (p) => {
  if (p.length > 0 && !showOutput.value) {
    showOutput.value = true;
    await store.loadOutput(props.id);
  }
});

function startPolling() {
  stopPolling();
  pollTimer = setInterval(async () => {
    await reload();
    // Stop polling once the task is no longer in a live state.
    if (!["pending", "running"].includes(store.detail?.status ?? "")) {
      stopPolling();
    }
  }, 2000);
}

function stopPolling() {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

onMounted(async () => {
  await reload();
  if (store.detail?.status === "running") startPolling();
});

watch(() => props.id, async (id) => {
  stopPolling();
  await reload();
  if (store.detail?.status === "running") startPolling();
});

watch(
  () => store.detail?.status,
  (s) => {
    if (s === "running" && pollTimer === null) startPolling();
    else if (s !== "running" && s !== "pending") stopPolling();
  },
);

onUnmounted(stopPolling);

const canKill = computed(() => store.detail?.status === "running");
const canRetry = computed(() =>
  ["failed", "completed", "killed"].includes(store.detail?.status ?? ""),
);
const canContinue = computed(() =>
  store.detail?.session_id && canRetry.value,
);
const isRunning = computed(() => store.detail?.status === "running");

async function withBusy(label: string, fn: () => Promise<void>) {
  busy.value = label;
  try {
    await fn();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = null;
  }
}

async function retry() {
  await withBusy("retry", async () => {
    const id = await store.retry(props.id);
    router.push({ name: "task-detail", params: { id } });
  });
}

async function continue_() {
  await withBusy("continue", async () => {
    // Resume reuses the same task row — just reload and re-arm the poller so
    // the streamed output keeps flowing.
    await store.continue_(props.id);
    await reload();
    if (store.detail?.status === "running" || store.detail?.status === "pending") {
      startPolling();
    }
  });
}

async function kill() {
  if (!confirm("Pause this task? Claude is stopped and the session id is kept so you can Resume later.")) return;
  await withBusy("kill", () => store.kill(props.id));
}

async function remove() {
  const msg = isRunning.value
    ? "Task is running. Force kill claude and delete?"
    : "Delete this task and its result?";
  if (!confirm(msg)) return;
  await withBusy("delete", async () => {
    await store.remove(props.id);
    router.push({ name: "tasks" });
  });
}

async function toggleOutput() {
  showOutput.value = !showOutput.value;
  if (showOutput.value) await store.loadOutput(props.id);
}

const showRaw = ref(false);
</script>

<template>
  <section v-if="store.detail" class="space-y-4">
    <header class="flex items-center gap-3">
      <ProviderBadge :provider="store.detail.provider" />
      <h1 class="text-xl font-semibold">{{ store.detail.project_path }}</h1>
      <StatusPill :status="store.detail.status" />
    </header>
    <dl class="grid grid-cols-2 gap-4 bg-white p-4 rounded shadow-sm text-sm">
      <div>
        <dt class="text-xs text-gray-500">Branch</dt>
        <dd>{{ store.detail.branch ?? store.detail.default_branch }}</dd>
      </div>
      <div>
        <dt class="text-xs text-gray-500">Trigger</dt>
        <dd>{{ store.detail.trigger_type }}</dd>
      </div>
      <div>
        <dt class="text-xs text-gray-500">Created</dt>
        <dd>{{ new Date(store.detail.created_at).toLocaleString() }}</dd>
      </div>
      <div>
        <dt class="text-xs text-gray-500">Finished</dt>
        <dd>
          {{
            store.detail.finished_at
              ? new Date(store.detail.finished_at).toLocaleString()
              : "—"
          }}
        </dd>
      </div>
      <div v-if="store.detail.pid !== null">
        <dt class="text-xs text-gray-500">PID</dt>
        <dd class="font-mono text-xs">{{ store.detail.pid }}</dd>
      </div>
      <div v-if="store.detail.session_id" class="col-span-2">
        <dt class="text-xs text-gray-500">Claude session</dt>
        <dd class="font-mono text-xs">{{ store.detail.session_id }}</dd>
      </div>
    </dl>

    <div class="flex flex-wrap gap-2">
      <button
        v-if="store.detail.status === 'pending'"
        :disabled="!!busy"
        class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        @click="store.confirm(props.id)"
      >
        Confirm &amp; run
      </button>
      <button
        v-if="canRetry"
        :disabled="!!busy"
        class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        @click="retry"
      >
        {{ busy === "retry" ? "Retrying…" : "Retry" }}
      </button>
      <button
        v-if="canContinue"
        :disabled="!!busy"
        class="rounded bg-emerald-600 text-white px-4 py-2 hover:bg-emerald-700 disabled:opacity-60"
        title="Resume the claude session that produced this task"
        @click="continue_"
      >
        {{ busy === "continue" ? "Resuming…" : "Resume" }}
      </button>
      <button
        v-if="canKill"
        :disabled="!!busy"
        class="rounded bg-amber-600 text-white px-4 py-2 hover:bg-amber-700 disabled:opacity-60"
        title="Stop claude. Session id is preserved so you can Resume later."
        @click="kill"
      >
        {{ busy === "kill" ? "Pausing…" : "Pause" }}
      </button>
      <button
        :disabled="!!busy"
        class="rounded border border-red-300 text-red-700 px-4 py-2 hover:bg-red-50 disabled:opacity-60 ml-auto"
        :title="isRunning ? 'Force-kill claude and delete' : 'Delete'"
        @click="remove"
      >
        {{
          busy === "delete"
            ? "Deleting…"
            : isRunning
              ? "Kill &amp; delete"
              : "Delete"
        }}
      </button>
    </div>

    <section v-if="store.detail.result" class="bg-white p-4 rounded shadow-sm space-y-2">
      <h2 class="font-medium">Result</h2>
      <dl class="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt class="text-xs text-gray-500">Cost</dt>
          <dd>${{ store.detail.result.cost_usd.toFixed(4) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-gray-500">Turns</dt>
          <dd>{{ store.detail.result.num_turns }}</dd>
        </div>
        <div>
          <dt class="text-xs text-gray-500">Tokens</dt>
          <dd>{{ store.detail.result.input_tokens }} / {{ store.detail.result.output_tokens }}</dd>
        </div>
      </dl>
      <div
        class="p-3 rounded"
        :class="store.detail.result.is_error ? 'bg-red-50' : 'bg-gray-50'"
      >
        <pre
          v-if="store.detail.result.is_error"
          class="text-xs whitespace-pre-wrap font-mono text-red-900"
        >{{ store.detail.result.result_text }}</pre>
        <MarkdownView
          v-else
          :source="store.detail.result.result_text"
        />
      </div>
    </section>

    <section class="bg-white p-4 rounded shadow-sm space-y-3">
      <h2 class="font-medium text-sm">Trigger</h2>
      <TriggerView :data="store.detail.trigger_data" />
      <details>
        <summary class="cursor-pointer text-[11px] text-gray-500">raw payload</summary>
        <pre class="text-xs whitespace-pre-wrap mt-2">{{
          JSON.stringify(store.detail.trigger_data, null, 2)
        }}</pre>
      </details>
    </section>

    <section class="bg-white p-4 rounded shadow-sm space-y-2">
      <button
        class="text-sm font-medium hover:text-blue-700"
        @click="toggleOutput"
      >
        {{ showOutput ? "▾" : "▸" }} Command output
        <span class="text-xs text-gray-500 ml-2">(in-memory; lost on agent restart)</span>
      </button>
      <div v-if="showOutput" class="space-y-2">
        <p v-if="!store.output" class="text-sm text-gray-500">
          No captured output for this task (evicted, never ran, or still running).
        </p>
        <template v-else>
          <div class="text-xs text-gray-500">
            <span v-if="!store.output.finished" class="text-blue-700">streaming…</span>
            <span v-else>Exit {{ store.output.exit_code ?? "?" }}</span>
            ·
            {{ new Date(store.output.captured_at).toLocaleString() }}
          </div>
          <details>
            <summary class="cursor-pointer text-xs text-gray-600">Command</summary>
            <pre class="text-xs whitespace-pre-wrap font-mono bg-gray-50 p-2 rounded">{{ store.output.command }}</pre>
          </details>
          <div>
            <div class="flex items-baseline gap-2 mb-1">
              <span class="text-xs uppercase text-gray-500">claude events</span>
              <button
                class="ml-auto text-[11px] text-gray-500 hover:text-gray-800"
                @click="showRaw = !showRaw"
              >
                {{ showRaw ? "show formatted" : "show raw json" }}
              </button>
            </div>
            <ClaudeStream
              v-if="!showRaw"
              :text="store.output.stdout"
              :pending="pendingApprovals"
              @resolved="onApprovalResolved"
            />
            <pre
              v-else
              class="text-xs whitespace-pre-wrap font-mono bg-gray-50 p-2 rounded max-h-96 overflow-auto"
            >{{ store.output.stdout || "(empty)" }}</pre>
          </div>
          <div v-if="store.output.stderr">
            <div class="text-xs uppercase text-gray-500 mb-1">stderr</div>
            <pre class="text-xs whitespace-pre-wrap font-mono bg-red-50 p-2 rounded max-h-64 overflow-auto">{{ store.output.stderr }}</pre>
          </div>
        </template>
      </div>
    </section>
  </section>
  <p v-else class="text-gray-500">Loading…</p>
</template>
