<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import { useServicesStore } from "../stores/services";
import TaskRow from "../components/TaskRow.vue";
import NewTaskModal from "../components/NewTaskModal.vue";
import { taskSpentSecs } from "../util/duration";
import { useTaskFilters } from "../util/taskFilters";
import type { BulkAction, Task } from "../types/api";

const store = useTasksStore();
const services = useServicesStore();
const router = useRouter();
// Two orthogonal server-side filters (taskState/agentState) and two client-side
// ones (serviceId/projectId). All four persist to localStorage so the operator
// returns to the same view across reloads.
const { taskState, agentState, serviceId, projectId } = useTaskFilters();
const newTaskOpen = ref(false);
// Single shared `now` so the whole table re-renders together; ticks once a
// second so still-accruing durations look live.
const now = ref(new Date());
let nowTimer: ReturnType<typeof setInterval> | null = null;

// --- Client-side column sort -------------------------------------------------
type SortKey =
  | "created"
  | "project"
  | "branch"
  | "trigger"
  | "task_state"
  | "agent_state"
  | "spent";
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
    case "task_state":
      return t.task_state ?? "";
    case "agent_state":
      return t.agent_state ?? "";
    case "spent":
      return taskSpentSecs(t, now.value);
  }
}

// --- Service / project filters ----------------------------------------------
// Service options come from the connected services; project options are derived
// from the tasks actually present, so the dropdown only lists projects with runs.
const serviceOptions = computed(() =>
  services.list.map((s) => ({ id: s.id, label: s.display_name })),
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
      (!serviceId.value || t.service_id === serviceId.value) &&
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

// --- Fast filters ------------------------------------------------------------
// One-click presets over the server-side state filters; "clear" also drops the
// client-side service/project filters for a full reset.
function fastFilter(kind: "running" | "pending" | "clear") {
  if (kind === "clear") {
    taskState.value = "";
    agentState.value = "";
    serviceId.value = "";
    projectId.value = "";
  } else if (kind === "running") {
    taskState.value = "";
    agentState.value = "running";
  } else {
    agentState.value = "";
    taskState.value = "pending";
  }
}

// --- Selection + bulk actions ------------------------------------------------
const selected = ref<Set<string>>(new Set());
const bulkBusy = ref<BulkAction | null>(null);

function toggle(id: string) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}

const selectedCount = computed(() => selected.value.size);
// Select-all reflects the *filtered* set, per the issue: "Select all (filtered)".
const allSelected = computed(
  () => sortedTasks.value.length > 0 && sortedTasks.value.every((t) => selected.value.has(t.id)),
);
const someSelected = computed(
  () => sortedTasks.value.some((t) => selected.value.has(t.id)) && !allSelected.value,
);

function toggleAll() {
  selected.value = allSelected.value
    ? new Set()
    : new Set(sortedTasks.value.map((t) => t.id));
}

function clearSelection() {
  selected.value = new Set();
}

const bulkLabels: Record<BulkAction, string> = {
  run: "Run",
  pause: "Pause",
  resume: "Resume",
  delete: "Delete",
};

async function runBulk(action: BulkAction) {
  const ids = Array.from(selected.value);
  if (!ids.length) return;
  if (action === "delete" && !confirm(`Delete ${ids.length} task(s)? This cannot be undone.`))
    return;
  if (action === "pause" && !confirm(`Pause ${ids.length} task(s)? Sessions are kept for resume.`))
    return;

  bulkBusy.value = action;
  try {
    const res = await store.bulk(action, ids);
    if (res.failed.length) {
      const detail = res.failed
        .slice(0, 5)
        .map((f) => `• ${f.error}`)
        .join("\n");
      const more = res.failed.length > 5 ? `\n…and ${res.failed.length - 5} more` : "";
      alert(`${res.succeeded.length} succeeded, ${res.failed.length} failed:\n${detail}${more}`);
    }
    clearSelection();
    await reload();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    bulkBusy.value = null;
  }
}

function activeFilters() {
  return {
    task_state: taskState.value || undefined,
    agent_state: agentState.value || undefined,
  };
}

const reload = () => store.refresh(activeFilters());

async function onCreated(id: string) {
  newTaskOpen.value = false;
  await reload();
  router.push({ name: "task-detail", params: { id } });
}

// Live poll: refresh silently every 10s so the table stays current without
// flashing the loading placeholder. Polls respect the active state filters.
let pollTimer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  reload();
  services.refresh();
  nowTimer = setInterval(() => (now.value = new Date()), 1000);
  pollTimer = setInterval(() => store.refresh(activeFilters(), true), 10_000);
});
onUnmounted(() => {
  if (nowTimer !== null) clearInterval(nowTimer);
  if (pollTimer !== null) clearInterval(pollTimer);
});
watch([taskState, agentState], reload);
// Drop selected ids that fell out of the loaded set (deleted, or filtered away
// server-side) so the bulk count never lies about what it'll act on.
watch(
  () => store.tasks,
  (list) => {
    const live = new Set(list.map((t) => t.id));
    if (![...selected.value].every((id) => live.has(id))) {
      selected.value = new Set([...selected.value].filter((id) => live.has(id)));
    }
  },
);
</script>

<template>
  <section>
    <div class="mb-6 flex items-start justify-between gap-4">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Tasks</h1>
        <p class="mt-1 text-sm text-muted">Agent runs across every connected repo.</p>
      </div>
      <div class="flex flex-wrap items-center justify-end gap-2">
        <div class="flex items-center gap-1">
          <button class="btn btn-subtle btn-sm" @click="fastFilter('running')">Running</button>
          <button class="btn btn-subtle btn-sm" @click="fastFilter('pending')">Pending</button>
          <button class="btn btn-subtle btn-sm" @click="fastFilter('clear')">Clear</button>
        </div>
        <select v-model="serviceId" class="select w-40">
          <option value="">All services</option>
          <option v-for="s in serviceOptions" :key="s.id" :value="s.id">{{ s.label }}</option>
        </select>
        <select v-model="projectId" class="select w-44">
          <option value="">All projects</option>
          <option v-for="p in projectOptions" :key="p.id" :value="p.id">{{ p.label }}</option>
        </select>
        <select v-model="taskState" class="select w-40">
          <option value="">All task states</option>
          <option value="pending">Pending</option>
          <option value="working_on">Working on</option>
          <option value="completed">Completed</option>
          <option value="failed">Failed</option>
        </select>
        <select v-model="agentState" class="select w-40">
          <option value="">All agent states</option>
          <option value="cold">Cold</option>
          <option value="warm">Warm</option>
          <option value="pending">Pending</option>
          <option value="running">Running</option>
          <option value="failed">Failed</option>
        </select>
        <button class="btn btn-primary" @click="newTaskOpen = true">+ New task</button>
      </div>
    </div>

    <NewTaskModal :open="newTaskOpen" @close="newTaskOpen = false" @created="onCreated" />

    <div
      v-if="selectedCount"
      class="card mb-3 flex flex-wrap items-center gap-2 px-4 py-2"
    >
      <span class="text-sm font-medium text-ink">{{ selectedCount }} selected</span>
      <span class="mx-1 h-4 w-px bg-line-2"></span>
      <button
        class="btn btn-subtle btn-sm text-accent"
        :disabled="!!bulkBusy"
        @click="runBulk('run')"
      >
        {{ bulkBusy === "run" ? "running…" : "Run" }}
      </button>
      <button
        class="btn btn-subtle btn-sm text-signal-release"
        :disabled="!!bulkBusy"
        @click="runBulk('pause')"
      >
        {{ bulkBusy === "pause" ? "pausing…" : "Pause" }}
      </button>
      <button
        class="btn btn-subtle btn-sm text-signal-ok"
        :disabled="!!bulkBusy"
        @click="runBulk('resume')"
      >
        {{ bulkBusy === "resume" ? "resuming…" : "Resume" }}
      </button>
      <button
        class="btn btn-subtle btn-sm text-signal-danger"
        :disabled="!!bulkBusy"
        @click="runBulk('delete')"
      >
        {{ bulkBusy === "delete" ? "deleting…" : "Delete" }}
      </button>
      <button class="btn btn-subtle btn-sm ml-auto" :disabled="!!bulkBusy" @click="clearSelection">
        Clear selection
      </button>
    </div>

    <div v-if="store.loading" class="card px-4 py-10 text-center text-muted">Loading…</div>
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th class="w-8">
              <input
                type="checkbox"
                class="cursor-pointer align-middle accent-accent"
                :checked="allSelected"
                :indeterminate="someSelected"
                aria-label="Select all filtered tasks"
                @change="toggleAll"
              />
            </th>
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
            <th class="cursor-pointer select-none" @click="sortBy('task_state')">
              Task<span v-if="sortKey === 'task_state'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none" @click="sortBy('agent_state')">
              Agent<span v-if="sortKey === 'agent_state'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="cursor-pointer select-none text-right" @click="sortBy('spent')">
              Spent<span v-if="sortKey === 'spent'"> {{ sortAsc ? "▲" : "▼" }}</span>
            </th>
            <th class="text-right">PID</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <TaskRow
            v-for="t in sortedTasks"
            :key="t.id"
            :task="t"
            :now="now"
            :selected="selected.has(t.id)"
            @changed="reload"
            @toggle="toggle(t.id)"
          />
          <tr v-if="!sortedTasks.length">
            <td colspan="11" class="py-10 text-center text-faint">
              {{ store.tasks.length ? "No tasks match these filters." : "No tasks yet." }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
