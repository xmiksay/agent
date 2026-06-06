<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import StatusPill from "../components/StatusPill.vue";
import ProviderBadge from "../components/ProviderBadge.vue";
import NewTaskModal from "../components/NewTaskModal.vue";
import { formatSecs, taskSpentSecs } from "../util/duration";
import { tasks as allTasks } from "../fixtures";
import type { Task } from "../types/api";

const router = useRouter();
const status = ref("");
const newTaskOpen = ref(false);
const now = new Date();

const tasks = computed<Task[]>(() =>
  status.value ? allTasks.filter((t) => t.status === status.value) : allTasks,
);

function spent(t: Task): string {
  return formatSecs(taskSpentSecs(t, now));
}
function triggerUrl(t: Task): string | null {
  const d = t.trigger_data as Record<string, unknown> | null;
  const u = d?.url;
  return typeof u === "string" && u.length > 0 ? u : null;
}
</script>

<template>
  <section>
    <div class="mb-6 flex flex-wrap items-center justify-between gap-3">
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

    <NewTaskModal :open="newTaskOpen" @close="newTaskOpen = false" @created="newTaskOpen = false" />

    <div class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Status</th><th>Provider</th><th>Project</th><th>Branch</th>
            <th>Trigger</th><th class="text-right">Spent</th><th class="text-right">PID</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="t in tasks" :key="t.id" class="cursor-pointer" @click="router.push(`/tasks/${t.id}`)">
            <td><StatusPill :status="t.status" /></td>
            <td><ProviderBadge :provider="t.provider" /></td>
            <td class="font-medium text-ink">{{ t.project_path }}</td>
            <td class="font-mono text-xs text-muted">{{ t.branch ?? "—" }}</td>
            <td class="text-xs">
              <a v-if="triggerUrl(t)" :href="triggerUrl(t)!" target="_blank" rel="noopener" class="text-accent hover:underline" @click.stop>{{ t.trigger_type }} ↗</a>
              <span v-else class="text-muted">{{ t.trigger_type }}</span>
            </td>
            <td class="text-right font-mono text-xs text-muted">{{ spent(t) }}</td>
            <td class="text-right font-mono text-xs text-faint">{{ t.pid ?? "—" }}</td>
          </tr>
          <tr v-if="!tasks.length">
            <td colspan="7" class="py-10 text-center text-faint">No tasks match.</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
