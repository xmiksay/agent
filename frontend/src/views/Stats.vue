<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { tasksApi } from "../api/tasks";
import { formatSecs } from "../util/duration";
import type { StatsGroupBy, StatsResponse } from "../types/api";

// Picks default to "last 30 days". `<input type="date">` works in local TZ;
// we convert to ISO UTC at the day boundary for the API call.
function isoMidnightUtc(localDate: string, endOfDay: boolean): string {
  // localDate is "yyyy-MM-dd" coming from <input type="date">.
  const [y, m, d] = localDate.split("-").map((n) => parseInt(n, 10));
  const base = new Date(Date.UTC(y, m - 1, d, endOfDay ? 23 : 0, endOfDay ? 59 : 0, 0, 0));
  return base.toISOString();
}

function todayLocal(offsetDays = 0): string {
  const d = new Date();
  d.setDate(d.getDate() + offsetDays);
  const yyyy = d.getFullYear();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}`;
}

const fromDate = ref<string>(todayLocal(-30));
const toDate = ref<string>(todayLocal());
const groupBy = ref<StatsGroupBy>("project");
const data = ref<StatsResponse | null>(null);
const loading = ref(false);
const errorMsg = ref<string | null>(null);

const groupOptions: { value: StatsGroupBy; label: string }[] = [
  { value: "project", label: "Project" },
  { value: "service", label: "Service" },
  { value: "branch", label: "Branch (+ project)" },
  { value: "trigger_type", label: "Trigger type" },
];

async function reload() {
  loading.value = true;
  errorMsg.value = null;
  try {
    data.value = await tasksApi.stats({
      from: isoMidnightUtc(fromDate.value, false),
      to: isoMidnightUtc(toDate.value, true),
      group_by: groupBy.value,
    });
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : String(e);
    data.value = null;
  } finally {
    loading.value = false;
  }
}

onMounted(reload);
watch([fromDate, toDate, groupBy], reload);

const totalLine = computed(() => {
  if (!data.value) return "";
  const { total_tasks, total_secs } = data.value;
  return `${total_tasks} task${total_tasks === 1 ? "" : "s"} · ${formatSecs(total_secs)} total`;
});

function quickRange(days: number) {
  toDate.value = todayLocal();
  fromDate.value = todayLocal(-days);
}
</script>

<template>
  <section class="space-y-4">
    <div class="flex items-end gap-4 flex-wrap">
      <div>
        <label class="block text-xs uppercase text-gray-500 mb-1">From</label>
        <input v-model="fromDate" type="date" class="border rounded px-2 py-1.5" />
      </div>
      <div>
        <label class="block text-xs uppercase text-gray-500 mb-1">To</label>
        <input v-model="toDate" type="date" class="border rounded px-2 py-1.5" />
      </div>
      <div>
        <label class="block text-xs uppercase text-gray-500 mb-1">Group by</label>
        <select v-model="groupBy" class="border rounded px-2 py-1.5">
          <option v-for="g in groupOptions" :key="g.value" :value="g.value">
            {{ g.label }}
          </option>
        </select>
      </div>
      <div class="flex gap-1.5 ml-auto">
        <button
          v-for="r in [
            { label: '7d', days: 7 },
            { label: '30d', days: 30 },
            { label: '90d', days: 90 },
          ]"
          :key="r.label"
          class="text-xs px-2 py-1 rounded border bg-white hover:bg-gray-50"
          @click="quickRange(r.days)"
        >
          {{ r.label }}
        </button>
      </div>
    </div>

    <p v-if="errorMsg" class="text-sm text-red-700">{{ errorMsg }}</p>

    <div v-if="loading" class="text-gray-500">Loading…</div>
    <div v-else-if="data">
      <p class="text-sm text-gray-600 mb-3">{{ totalLine }}</p>
      <table class="tbl">
        <thead>
          <tr>
            <th>{{ groupOptions.find((g) => g.value === groupBy)?.label }}</th>
            <th class="text-right">Tasks</th>
            <th class="text-right">Spent</th>
            <th class="text-right">Share</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in data.rows" :key="row.key">
            <td class="text-sm">{{ row.label }}</td>
            <td class="text-right font-mono text-sm">{{ row.task_count }}</td>
            <td class="text-right font-mono text-sm">{{ formatSecs(row.total_secs) }}</td>
            <td class="text-right font-mono text-xs text-gray-500">
              {{
                data.total_secs > 0
                  ? Math.round((row.total_secs / data.total_secs) * 100) + "%"
                  : "—"
              }}
            </td>
          </tr>
          <tr v-if="!data.rows.length">
            <td colspan="4" class="px-3 py-6 text-center text-gray-500">
              No tasks in this window.
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
