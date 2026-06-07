<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import { useGitServicesStore } from "../stores/git_services";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import NewTaskModal from "../components/NewTaskModal.vue";
import { formatSecs, taskSpentSecs } from "../util/duration";
import type { Task } from "../types/api";

const store = useTasksStore();
const gitServices = useGitServicesStore();
const router = useRouter();
const status = ref("");
// Service + project filters are applied client-side over the loaded set, so
// changing them is instant and never refetches.
const serviceId = ref("");
const projectId = ref("");
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

// --- Client-side column sort -------------------------------------------------
type SortKey = "created" | "project" | "branch" | "trigger" | "status" | "spent";
const sortKey = ref<SortKey>("created");
const sortAsc = ref(false); // default: newest first

function sortBy(key: SortKey) {
  if (sortKey.value === key) {
    sortAsc.value = !sortAsc.value;
  } else {
    sortKey.value = key;
    sortAsc.value = true;
  }
}

function sortValue(t: Task, key: SortKey): string | number {
  switch (key) {
    case "created":
      return new Date(t.created_at).getTime();
    case "project":
      return t.project_path ?? "";
    case "branch":
      return t.branch ?? "";
    case "trigger":
      return t.trigger_type ?? "";
    case "status":
      return t.status ?? "";
    case "spent":
      return taskSpentSecs(t, now.value);
  }
}

// --- Service / project filters ----------------------------------------------
// Service options come from the connected services; project options are derived
// from the tasks actually present, so the dropdown only lists projects with runs.
const serviceOptions = computed(() =>
  gitServices.list.map((s) => ({ id: s.id, label: s.display_name })),
);
const projectOptions = computed(() => {
  const seen = new Map<string, string>();
  for (const t of store.tasks) {
    if (t.project_id && !seen.has(t.project_id)) seen.set(t.project_id, t.project_path);
  }
  return [...seen].map(([id, label]) => ({ id, label })).sort((a, b) =>
    a.label.localeCompare(b.label),
  );
});

const filteredTasks = computed(() =>
  store.tasks.filter(
    (t) =>
      (!serviceId.value || t.git_service_id === serviceId.value) &&
      (!projectId.value || t.project_id === projectId.value),
  ),
);

const sortedTasks = computed(() => {
  const dir = sortAsc.value ? 1 : -1;
  return [...filteredTasks.value].sort((a, b) => {
    const av = sortValue(a, sortKey.value);
    const bv = sortValue(b, sortKey.value);
    if (av < bv) return -1 * dir;
    if (av > bv) return 1 * dir;
    return 0;
  });
});

async function onCreated(id: string) {
  newTaskOpen.value = false;
  await reload();
  router.push({ name: "task-detail", params: { id } });
}

const reload = () => store.refresh(status.value || undefined);
// Live poll: refresh silently every 10s so the table stays current without
// flashing the loading placeholder.
let pollTimer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  reload();
  gitServices.refresh();
  nowTimer = setInterval(() => (now.value = new Date()), 1000);
  pollTimer = setInterval(() => store.refresh(status.value || undefined, true), 10_000);
});
onUnmounted(() => {
  if (nowTimer !== null) clearInterval(nowTimer);
  if (pollTimer !== null) clearInterval(pollTimer);
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
        <select v-model="serviceId" class="select w-40">
          <option value="">All services</option>
          <option v-for="s in serviceOptions" :key="s.id" :value="s.id">{{ s.label }}</option>
        </select>
        <select v-model="projectId" class="select w-44">
          <option value="">All projects</option>
          <option v-for="p in projectOptions" :key="p.id" :value="p.id">{{ p.label }}</option>
        </select>
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
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th class="cursor-pointer select-none" @click="sortBy('created')">
              Created<span v-if="sortKey === 'created'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th>Provider</th>
            <th class="cursor-pointer select-none" @click="sortBy('project')">
              Project<span v-if="sortKey === 'project'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none" @click="sortBy('branch')">
              Branch<span v-if="sortKey === 'branch'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none" @click="sortBy('trigger')">
              Trigger<span v-if="sortKey === 'trigger'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none" @click="sortBy('status')">
              Status<span v-if="sortKey === 'status'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none text-right" @click="sortBy('spent')">
              Spent<span v-if="sortKey === 'spent'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="text-right">PID</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="t in sortedTasks"
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
          <tr v-if="!sortedTasks.length">
            <td colspan="9" class="py-10 text-center text-faint">
              {{ store.tasks.length ? "No tasks match these filters." : "No tasks yet." }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
