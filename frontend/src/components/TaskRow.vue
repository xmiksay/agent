<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { useTasksStore } from "../stores/tasks";
import StatusPill from "./StatusPill.vue";
import ProviderBadge from "./ProviderBadge.vue";
import { formatSecs, taskSpentSecs } from "../util/duration";
import type { Task } from "../types/api";

// One task row. Owns its own busy + inline-edit state (only one action runs per
// row at a time) and talks to the store directly; the parent just re-fetches the
// list when something changed.
const props = defineProps<{ task: Task; now: Date; selected: boolean }>();
const emit = defineEmits<{ (e: "changed"): void; (e: "toggle"): void }>();

const store = useTasksStore();
const router = useRouter();
const busy = ref<string | null>(null);

// Liveness is "an agent is attached" — running or warm (idle between turns).
const isLive = (t: Task) => t.agent_state === "running" || t.agent_state === "warm";

function spent(t: Task): string {
  return formatSecs(taskSpentSecs(t, props.now));
}

// Compact, fixed-footprint timestamp so the column never forces the table wide.
function created(t: Task): string {
  const d = new Date(t.created_at);
  const day = d.toLocaleDateString([], { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `${day} ${time}`;
}

// trigger_data is a serialized TriggerReason; every variant carries `url`.
function triggerUrl(t: Task): string | null {
  const d = t.trigger_data;
  if (!d || typeof d !== "object") return null;
  const u = (d as Record<string, unknown>).url;
  return typeof u === "string" && u.length > 0 ? u : null;
}

function openTask() {
  router.push({ name: "task-detail", params: { id: props.task.id } });
}

function canResume(t: Task) {
  return (
    !!t.session_id && (t.agent_state === "cold" || t.agent_state === "failed")
  );
}

async function withBusy(action: string, fn: () => Promise<void>) {
  busy.value = action;
  try {
    await fn();
  } catch (e) {
    alert(e instanceof Error ? e.message : String(e));
  } finally {
    busy.value = null;
  }
}

async function remove() {
  const msg = isLive(props.task)
    ? `Task on ${props.task.project_path} has a live agent. Force kill claude and delete?`
    : `Delete task on ${props.task.project_path}?`;
  if (!confirm(msg)) return;
  await withBusy("delete", () => store.remove(props.task.id));
}

async function pause() {
  if (!confirm("Pause this task? Session id is kept so you can Resume later.")) return;
  await withBusy("pause", async () => {
    await store.kill(props.task.id);
    emit("changed");
  });
}

async function resume() {
  await withBusy("resume", async () => {
    const newId = await store.continue_(props.task.id);
    router.push({ name: "task-detail", params: { id: newId } });
  });
}

async function confirmRun() {
  await withBusy("confirm", async () => {
    await store.confirm(props.task.id);
    emit("changed");
  });
}

// --- Inline branch edit (pending tasks only) ---------------------------------
const editing = ref(false);
const editBranch = ref("");

function startEdit() {
  editing.value = true;
  editBranch.value = props.task.branch ?? "";
}
function cancelEdit() {
  editing.value = false;
}
async function saveEdit() {
  await withBusy("edit", async () => {
    await store.update(props.task.id, { branch: editBranch.value.trim() || undefined });
    editing.value = false;
    emit("changed");
  });
}
</script>

<template>
  <tr class="cursor-pointer" :class="selected ? 'bg-accent/5' : ''" @click="openTask">
    <td class="w-8" @click.stop>
      <input
        type="checkbox"
        class="cursor-pointer align-middle accent-accent"
        :checked="selected"
        aria-label="Select task"
        @change="emit('toggle')"
      />
    </td>
    <td class="whitespace-nowrap text-xs text-faint">{{ created(task) }}</td>
    <td><ProviderBadge :provider="task.provider" /></td>
    <td class="max-w-[220px]">
      <RouterLink
        v-if="task.project_id"
        :to="`/projects/${task.project_id}`"
        class="block truncate font-medium text-ink hover:text-accent"
        :title="task.project_path"
        @click.stop
        >{{ task.project_path }}</RouterLink
      >
      <span v-else class="block truncate font-medium text-ink" :title="task.project_path">{{
        task.project_path
      }}</span>
    </td>
    <td class="max-w-[200px]">
      <div v-if="editing" class="flex items-center gap-1.5" @click.stop>
        <input
          v-model="editBranch"
          :disabled="busy === 'edit'"
          class="input w-40 px-2 py-1 font-mono text-xs"
          placeholder="branch"
          @keyup.enter="saveEdit"
          @keyup.esc="cancelEdit"
        />
        <button :disabled="busy === 'edit'" class="btn btn-subtle btn-sm text-accent" @click="saveEdit">
          {{ busy === "edit" ? "…" : "save" }}
        </button>
        <button :disabled="busy === 'edit'" class="btn btn-subtle btn-sm" @click="cancelEdit">cancel</button>
      </div>
      <span v-else class="flex items-center gap-1.5">
        <span class="truncate font-mono text-xs text-muted" :title="task.branch ?? ''">{{
          task.branch ?? "—"
        }}</span>
        <button
          v-if="task.task_state === 'pending'"
          class="btn btn-subtle btn-sm shrink-0 text-accent"
          title="Edit branch"
          @click.stop="startEdit"
        >
          edit
        </button>
      </span>
    </td>
    <td class="text-xs">
      <a
        v-if="triggerUrl(task)"
        :href="triggerUrl(task) ?? undefined"
        target="_blank"
        rel="noopener"
        class="text-accent hover:underline"
        :title="triggerUrl(task) ?? ''"
        @click.stop
      >
        {{ task.trigger_type }} ↗
      </a>
      <span v-else class="text-muted">{{ task.trigger_type }}</span>
    </td>
    <td><StatusPill :status="task.task_state" /></td>
    <td><StatusPill :status="task.agent_state" /></td>
    <td class="whitespace-nowrap text-right font-mono text-xs text-muted">{{ spent(task) }}</td>
    <td class="text-right font-mono text-xs text-faint">{{ task.pid ?? "—" }}</td>
    <td class="whitespace-nowrap text-right">
      <div class="inline-flex items-center gap-1.5" @click.stop>
        <button
          v-if="task.task_state === 'pending' && task.agent_state === 'cold'"
          :disabled="!!busy"
          class="btn btn-subtle btn-sm text-accent"
          @click="confirmRun"
        >
          {{ busy === "confirm" ? "starting…" : "run" }}
        </button>
        <button
          v-if="isLive(task)"
          :disabled="!!busy"
          class="btn btn-subtle btn-sm text-signal-release"
          title="Pause: kill claude but keep session id for Resume"
          @click="pause"
        >
          {{ busy === "pause" ? "pausing…" : "pause" }}
        </button>
        <button
          v-if="canResume(task)"
          :disabled="!!busy"
          class="btn btn-subtle btn-sm text-signal-ok"
          title="Resume claude using its prior session id"
          @click="resume"
        >
          {{ busy === "resume" ? "resuming…" : "resume" }}
        </button>
        <button
          :disabled="!!busy"
          class="btn btn-subtle btn-sm text-signal-danger"
          :title="isLive(task) ? 'Force-kill claude and delete' : 'Delete'"
          @click="remove"
        >
          {{ busy === "delete" ? "deleting…" : isLive(task) ? "kill & delete" : "delete" }}
        </button>
      </div>
    </td>
  </tr>
</template>
