<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import NewTaskModal from "../components/NewTaskModal.vue";
import { formatSecs, taskSpentSecs } from "../util/duration";
import type { Task } from "../types/api";

const store = useTasksStore();
const router = useRouter();
const status = ref("");
const busy = ref<{ id: string; action: string } | null>(null);
const newTaskOpen = ref(false);
// Single shared `now` so the whole table re-renders together; ticks once a
// second so running-task durations look live.
const now = ref(new Date());
let nowTimer: ReturnType<typeof setInterval> | null = null;
function spent(t: Task): string {
  return formatSecs(taskSpentSecs(t, now.value));
}

// trigger_data is a serialized TriggerReason; every variant carries `url`.
function triggerUrl(t: Task): string | null {
  const d = t.trigger_data;
  if (!d || typeof d !== "object") return null;
  const u = (d as Record<string, unknown>).url;
  return typeof u === "string" && u.length > 0 ? u : null;
}

async function onCreated(id: string) {
  newTaskOpen.value = false;
  await reload();
  router.push({ name: "task-detail", params: { id } });
}

const reload = () => store.refresh(status.value || undefined);

onMounted(() => {
  reload();
  nowTimer = setInterval(() => (now.value = new Date()), 1000);
});
onUnmounted(() => {
  if (nowTimer !== null) clearInterval(nowTimer);
});
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

// --- Inline branch edit (pending tasks only) ---------------------------------
const editingId = ref<string | null>(null);
const editBranch = ref("");

function startEdit(t: Task) {
  editingId.value = t.id;
  editBranch.value = t.branch ?? "";
}
function cancelEdit() {
  editingId.value = null;
}
async function saveEdit(t: Task) {
  setBusy(t.id, "edit");
  try {
    await store.update(t.id, { branch: editBranch.value.trim() || undefined });
    editingId.value = null;
    await reload();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    clearBusy();
  }
}
</script>

<template>
  <section>
    <div class="flex items-center justify-between mb-4">
      <h1 class="text-2xl font-semibold">Tasks</h1>
      <div class="flex items-center gap-2">
        <button
          class="px-3 py-1 text-sm rounded bg-blue-600 text-white hover:bg-blue-700"
          @click="newTaskOpen = true"
        >
          New task
        </button>
        <select v-model="status" class="border rounded px-2 py-1 text-sm">
          <option value="">All</option>
          <option value="pending">Pending</option>
          <option value="awaiting_auth">Awaiting auth</option>
          <option value="running">Running</option>
          <option value="completed">Completed</option>
          <option value="failed">Failed</option>
        </select>
      </div>
    </div>
    <NewTaskModal :open="newTaskOpen" @close="newTaskOpen = false" @created="onCreated" />
    <div v-if="store.loading" class="text-gray-500">Loading…</div>
    <table v-else class="tbl">
      <thead>
        <tr>
          <th>Created</th>
          <th>Provider</th>
          <th>Project</th>
          <th>Branch</th>
          <th>Trigger</th>
          <th>Status</th>
          <th>Spent</th>
          <th>PID</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="t in store.tasks" :key="t.id">
          <td class="text-xs text-gray-500 whitespace-nowrap">
            {{ new Date(t.created_at).toLocaleString() }}
          </td>
          <td><ProviderBadge :provider="t.provider" /></td>
          <td>
            <RouterLink :to="`/tasks/${t.id}`">{{ t.project_path }}</RouterLink>
          </td>
          <td class="text-sm text-gray-600">
            <div v-if="editingId === t.id" class="flex items-center gap-1">
              <input
                v-model="editBranch"
                :disabled="busyOn(t.id, 'edit')"
                class="w-40 text-xs font-mono border border-gray-300 rounded px-1.5 py-1 disabled:opacity-60"
                placeholder="branch"
                @keyup.enter="saveEdit(t)"
                @keyup.esc="cancelEdit"
              />
              <button
                :disabled="busyOn(t.id, 'edit')"
                class="text-xs text-blue-700 hover:underline disabled:opacity-60"
                @click="saveEdit(t)"
              >
                {{ busyOn(t.id, "edit") ? "…" : "save" }}
              </button>
              <button
                :disabled="busyOn(t.id, 'edit')"
                class="text-xs text-gray-500 hover:underline disabled:opacity-60"
                @click="cancelEdit"
              >
                cancel
              </button>
            </div>
            <span v-else class="inline-flex items-center gap-1.5">
              <span class="font-mono">{{ t.branch ?? "—" }}</span>
              <button
                v-if="t.status === 'pending'"
                class="text-xs text-blue-700 hover:underline"
                title="Edit branch"
                @click="startEdit(t)"
              >
                edit
              </button>
            </span>
          </td>
          <td class="text-sm">
            <a
              v-if="triggerUrl(t)"
              :href="triggerUrl(t) ?? undefined"
              target="_blank"
              rel="noopener"
              class="text-blue-700 hover:underline"
              :title="triggerUrl(t) ?? ''"
              @click.stop
            >
              {{ t.trigger_type }} ↗
            </a>
            <template v-else>{{ t.trigger_type }}</template>
          </td>
          <td><StatusPill :status="t.status" /></td>
          <td class="text-xs font-mono text-gray-700 whitespace-nowrap">{{ spent(t) }}</td>
          <td class="font-mono text-xs text-gray-600">{{ t.pid ?? "—" }}</td>
          <td class="text-right whitespace-nowrap">
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
          <td colspan="9" class="px-3 py-6 text-center text-gray-500">No tasks yet.</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
