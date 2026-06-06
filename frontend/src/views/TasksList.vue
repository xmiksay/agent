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

// Compact, fixed-footprint timestamp so the column never forces the table wide.
function created(t: Task): string {
  const d = new Date(t.created_at);
  const day = d.toLocaleDateString([], { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `${day} ${time}`;
}

function openTask(t: Task) {
  router.push({ name: "task-detail", params: { id: t.id } });
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
    <div class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Tasks</h1>
        <p class="mt-1 text-sm text-muted">Agent runs across every connected repo.</p>
      </div>
      <div class="flex items-center gap-2">
        <select v-model="status" class="select w-40">
          <option value="">All statuses</option>
          <option value="pending">Pending</option>
          <option value="awaiting_auth">Awaiting auth</option>
          <option value="running">Running</option>
          <option value="completed">Completed</option>
          <option value="failed">Failed</option>
        </select>
        <button class="btn btn-primary" @click="newTaskOpen = true">+ New task</button>
      </div>
    </div>

    <NewTaskModal :open="newTaskOpen" @close="newTaskOpen = false" @created="onCreated" />

    <div v-if="store.loading" class="card px-4 py-10 text-center text-muted">Loading…</div>
    <div v-else class="card overflow-hidden">
      <table class="tbl">
        <thead>
          <tr>
            <th>Created</th>
            <th>Provider</th>
            <th>Project</th>
            <th>Branch</th>
            <th>Trigger</th>
            <th>Status</th>
            <th class="text-right">Spent</th>
            <th class="text-right">PID</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="t in store.tasks"
            :key="t.id"
            class="cursor-pointer"
            @click="openTask(t)"
          >
            <td class="whitespace-nowrap text-xs text-faint">
              {{ created(t) }}
            </td>
            <td><ProviderBadge :provider="t.provider" /></td>
            <td class="max-w-[220px]">
              <RouterLink
                v-if="t.project_id"
                :to="`/projects/${t.project_id}`"
                class="block truncate font-medium text-ink hover:text-accent"
                :title="t.project_path"
                @click.stop
                >{{ t.project_path }}</RouterLink
              >
              <span v-else class="block truncate font-medium text-ink" :title="t.project_path">{{
                t.project_path
              }}</span>
            </td>
            <td class="max-w-[200px]">
              <div v-if="editingId === t.id" class="flex items-center gap-1.5" @click.stop>
                <input
                  v-model="editBranch"
                  :disabled="busyOn(t.id, 'edit')"
                  class="input w-40 px-2 py-1 font-mono text-xs"
                  placeholder="branch"
                  @keyup.enter="saveEdit(t)"
                  @keyup.esc="cancelEdit"
                />
                <button
                  :disabled="busyOn(t.id, 'edit')"
                  class="btn btn-subtle btn-sm text-accent"
                  @click="saveEdit(t)"
                >
                  {{ busyOn(t.id, "edit") ? "…" : "save" }}
                </button>
                <button
                  :disabled="busyOn(t.id, 'edit')"
                  class="btn btn-subtle btn-sm"
                  @click="cancelEdit"
                >
                  cancel
                </button>
              </div>
              <span v-else class="flex items-center gap-1.5">
                <span class="truncate font-mono text-xs text-muted" :title="t.branch ?? ''">{{
                  t.branch ?? "—"
                }}</span>
                <button
                  v-if="t.status === 'pending'"
                  class="btn btn-subtle btn-sm shrink-0 text-accent"
                  title="Edit branch"
                  @click.stop="startEdit(t)"
                >
                  edit
                </button>
              </span>
            </td>
            <td class="text-xs">
              <a
                v-if="triggerUrl(t)"
                :href="triggerUrl(t) ?? undefined"
                target="_blank"
                rel="noopener"
                class="text-accent hover:underline"
                :title="triggerUrl(t) ?? ''"
                @click.stop
              >
                {{ t.trigger_type }} ↗
              </a>
              <span v-else class="text-muted">{{ t.trigger_type }}</span>
            </td>
            <td><StatusPill :status="t.status" /></td>
            <td class="whitespace-nowrap text-right font-mono text-xs text-muted">{{ spent(t) }}</td>
            <td class="text-right font-mono text-xs text-faint">{{ t.pid ?? "—" }}</td>
            <td class="whitespace-nowrap text-right">
              <div class="inline-flex items-center gap-1.5" @click.stop>
                <button
                  v-if="t.status === 'pending'"
                  :disabled="busyOn(t.id)"
                  class="btn btn-subtle btn-sm text-accent"
                  @click="confirmRun(t)"
                >
                  {{ busyOn(t.id, "confirm") ? "starting…" : "run" }}
                </button>
                <button
                  v-if="t.status === 'running'"
                  :disabled="busyOn(t.id)"
                  class="btn btn-subtle btn-sm text-signal-release"
                  title="Pause: kill claude but keep session id for Resume"
                  @click="pause(t)"
                >
                  {{ busyOn(t.id, "pause") ? "pausing…" : "pause" }}
                </button>
                <button
                  v-if="canResume(t)"
                  :disabled="busyOn(t.id)"
                  class="btn btn-subtle btn-sm text-signal-ok"
                  title="Resume claude using its prior session id"
                  @click="resume(t)"
                >
                  {{ busyOn(t.id, "resume") ? "resuming…" : "resume" }}
                </button>
                <button
                  :disabled="busyOn(t.id)"
                  class="btn btn-subtle btn-sm text-signal-danger"
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
            <td colspan="9" class="py-10 text-center text-faint">No tasks yet.</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
