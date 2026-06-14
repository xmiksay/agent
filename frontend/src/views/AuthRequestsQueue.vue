<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useStreamStore } from "../stores/stream";
import { useTasksStore } from "../stores/tasks";
import { authApi } from "../api/auth";
import StatusPill from "../components/StatusPill.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import type { AuthRequest, Task } from "../types/api";

// Approvals stream in live over the single app-wide socket (the same store that
// powers the nav badge). Seed once from REST to cover any raised before this
// client connected — no polling.
const stream = useStreamStore();
const tasks = useTasksStore();

onMounted(async () => {
  try {
    stream.seedApprovals(await authApi.list({ status: "pending" }));
  } catch {
    /* ignore — list just stays as whatever the socket has delivered */
  }
  // Task context (project / trigger / origin link) for each pending approval.
  tasks.refresh().catch(() => {
    /* ignore — approvals still render without task context */
  });
});

const taskById = computed(() => {
  const m = new Map<string, Task>();
  for (const t of tasks.tasks) m.set(t.id, t);
  return m;
});

// trigger_data is a serialized TriggerReason; every variant carries `url`.
function triggerUrl(t: Task): string | null {
  const d = t.trigger_data;
  if (!d || typeof d !== "object") return null;
  const u = (d as Record<string, unknown>).url;
  return typeof u === "string" && u.length > 0 ? u : null;
}

const pending = computed<AuthRequest[]>(() =>
  [...stream.approvals.values()]
    .filter((a) => a.status === "pending")
    .sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()),
);

function onResolved(r: AuthRequest) {
  stream.dropApproval(r.id);
}

const denyingAll = ref(false);
const bulkError = ref<string | null>(null);

// Deny every pending request in one call — the escape hatch when the queue has
// filled with stale asks. The backend publishes a resolution per row, so the
// socket clears them; we also drop them locally so the list empties instantly.
async function denyAll() {
  if (!pending.value.length || denyingAll.value) return;
  if (!window.confirm(`Deny all ${pending.value.length} pending requests?`)) return;
  denyingAll.value = true;
  bulkError.value = null;
  const ids = pending.value.map((r) => r.id);
  try {
    await authApi.bulkResolve({ all_pending: true, decision: "deny" });
    for (const id of ids) stream.dropApproval(id);
  } catch (e) {
    bulkError.value = e instanceof Error ? e.message : String(e);
  } finally {
    denyingAll.value = false;
  }
}
</script>

<template>
  <section>
    <div class="mb-6 flex items-center gap-2">
      <h1 class="font-display text-2xl font-bold tracking-tight">Pending operator approvals</h1>
      <span v-if="pending.length" class="pill text-accent">
        <span class="led text-accent" /> {{ pending.length }} pending
      </span>
      <button
        v-if="pending.length"
        :disabled="denyingAll"
        class="btn btn-danger btn-sm ml-auto"
        @click="denyAll"
      >
        {{ denyingAll ? "Denying…" : "Deny all" }}
      </button>
    </div>
    <p v-if="bulkError" class="mb-3 text-xs text-signal-danger">{{ bulkError }}</p>

    <div v-if="!pending.length" class="card p-10 text-center text-faint">
      <span class="led mx-auto mb-2 block w-fit text-signal-ok" />
      No pending requests.
    </div>

    <ul v-else class="space-y-3">
      <li v-for="r in pending" :key="r.id" class="card space-y-2 border-accent/30 p-4">
        <div class="flex items-center gap-2 text-xs text-faint">
          <span class="font-mono">{{ r.id.slice(0, 8) }}</span>
          <StatusPill :status="r.status" />
          <span class="font-mono">{{ new Date(r.created_at).toLocaleTimeString() }}</span>
          <RouterLink :to="`/tasks/${r.task_id}`" class="ml-auto">task</RouterLink>
        </div>

        <div
          v-if="taskById.get(r.task_id)"
          class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
        >
          <span class="font-medium text-ink">{{ taskById.get(r.task_id)!.project_path }}</span>
          <a
            v-if="triggerUrl(taskById.get(r.task_id)!)"
            :href="triggerUrl(taskById.get(r.task_id)!) ?? undefined"
            target="_blank"
            rel="noopener"
            class="text-accent hover:underline"
            :title="triggerUrl(taskById.get(r.task_id)!) ?? ''"
          >
            {{ taskById.get(r.task_id)!.trigger_type }} ↗
          </a>
          <span v-else class="text-muted">{{ taskById.get(r.task_id)!.trigger_type }}</span>
        </div>
        <pre class="whitespace-pre-wrap rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-ink">{{ r.requested_op }}</pre>
        <p class="text-sm text-muted">{{ r.prompt_to_operator }}</p>
        <AuthApprovalForm :item="r" compact @resolved="onResolved" />
      </li>
    </ul>
  </section>
</template>
