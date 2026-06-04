<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import type { Task } from "../types/api";

const store = useTasksStore();
const router = useRouter();
const status = ref("");
const busy = ref<{ id: string; action: string } | null>(null);

const reload = () => store.refresh(status.value || undefined);

onMounted(reload);
watch(status, reload);

function setBusy(id: string, action: string) {
  busy.value = { id, action };
}
function clearBusy() {
  busy.value = null;
}
function busyOn(id: string, action?: string) {
  return busy.value?.id === id && (!action || busy.value.action === action);
}

async function remove(t: Task) {
  const msg =
    t.status === "running"
      ? `Task on ${t.project_path} is running. Force kill claude and delete?`
      : `Delete task on ${t.project_path}?`;
  if (!confirm(msg)) return;
  setBusy(t.id, "delete");
  try {
    await store.remove(t.id);
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    clearBusy();
  }
}

async function pause(t: Task) {
  if (!confirm("Pause this task? Session id is kept so you can Resume later.")) return;
  setBusy(t.id, "pause");
  try {
    await store.kill(t.id);
    await reload();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    clearBusy();
  }
}

async function resume(t: Task) {
  setBusy(t.id, "resume");
  try {
    const newId = await store.continue_(t.id);
    router.push({ name: "task-detail", params: { id: newId } });
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    clearBusy();
  }
}

async function confirmRun(t: Task) {
  setBusy(t.id, "confirm");
  try {
    await store.confirm(t.id);
    await reload();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    clearBusy();
  }
}

function canResume(t: Task) {
  return (
    !!t.session_id &&
    ["failed", "completed", "killed"].includes(t.status)
  );
}
</script>

<template>
  <section>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-semibold">Tasks</h1>
      <select v-model="status" class="border rounded px-2 py-1 text-sm">
        <option value="">All</option>
        <option value="pending">Pending</option>
        <option value="awaiting_auth">Awaiting auth</option>
        <option value="running">Running</option>
        <option value="completed">Completed</option>
        <option value="failed">Failed</option>
      </select>
    </div>
    <div v-if="store.loading" class="text-gray-500">Loading…</div>
    <table v-else class="min-w-full bg-white rounded shadow-sm">
      <thead class="text-left text-xs uppercase text-gray-500 border-b">
        <tr>
          <th class="px-3 py-2">Created</th>
          <th class="px-3 py-2">Provider</th>
          <th class="px-3 py-2">Project</th>
          <th class="px-3 py-2">Branch</th>
          <th class="px-3 py-2">Trigger</th>
          <th class="px-3 py-2">Status</th>
          <th class="px-3 py-2">PID</th>
          <th class="px-3 py-2"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="t in store.tasks" :key="t.id" class="border-b last:border-0">
          <td class="px-3 py-2 text-xs text-gray-500">
            {{ new Date(t.created_at).toLocaleString() }}
          </td>
          <td class="px-3 py-2"><ProviderBadge :provider="t.provider" /></td>
          <td class="px-3 py-2">
            <RouterLink :to="`/tasks/${t.id}`">{{ t.project_path }}</RouterLink>
          </td>
          <td class="px-3 py-2 text-sm text-gray-600">{{ t.branch ?? "—" }}</td>
          <td class="px-3 py-2 text-sm">{{ t.trigger_type }}</td>
          <td class="px-3 py-2"><StatusPill :status="t.status" /></td>
          <td class="px-3 py-2 font-mono text-xs text-gray-600">{{ t.pid ?? "—" }}</td>
          <td class="px-3 py-2 text-right">
            <div class="inline-flex gap-3 items-center">
              <button
                v-if="t.status === 'pending'"
                :disabled="busyOn(t.id)"
                class="text-xs text-blue-700 hover:underline disabled:opacity-60"
                @click="confirmRun(t)"
              >
                {{ busyOn(t.id, "confirm") ? "starting…" : "run" }}
              </button>
              <button
                v-if="t.status === 'running'"
                :disabled="busyOn(t.id)"
                class="text-xs text-amber-700 hover:underline disabled:opacity-60"
                title="Pause: kill claude but keep session id for Resume"
                @click="pause(t)"
              >
                {{ busyOn(t.id, "pause") ? "pausing…" : "pause" }}
              </button>
              <button
                v-if="canResume(t)"
                :disabled="busyOn(t.id)"
                class="text-xs text-emerald-700 hover:underline disabled:opacity-60"
                title="Resume claude using its prior session id"
                @click="resume(t)"
              >
                {{ busyOn(t.id, "resume") ? "resuming…" : "resume" }}
              </button>
              <button
                :disabled="busyOn(t.id)"
                class="text-xs text-red-600 hover:underline disabled:opacity-60"
                :title="t.status === 'running' ? 'Force-kill claude and delete' : 'Delete'"
                @click="remove(t)"
              >
                {{
                  busyOn(t.id, "delete")
                    ? "deleting…"
                    : t.status === "running"
                      ? "kill & delete"
                      : "delete"
                }}
              </button>
            </div>
          </td>
        </tr>
        <tr v-if="!store.tasks.length">
          <td colspan="8" class="px-3 py-6 text-center text-gray-500">No tasks yet.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
