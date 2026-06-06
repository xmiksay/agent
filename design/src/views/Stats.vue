<script setup lang="ts">
import { computed } from "vue";
import { formatSecs } from "../util/duration";
import { stats } from "../fixtures";

const max = computed(() => Math.max(...stats.rows.map((r) => r.total_secs), 1));
</script>

<template>
  <section>
    <div class="mb-6">
      <h1 class="font-display text-2xl font-bold tracking-tight">Stats</h1>
      <p class="mt-1 text-sm text-muted">Time spent, grouped by project · last 30 days.</p>
    </div>

    <div class="mb-6 grid gap-3 sm:grid-cols-3">
      <div class="card p-4">
        <p class="label">Total tasks</p>
        <p class="mt-1 font-display text-3xl font-bold">{{ stats.total_tasks }}</p>
      </div>
      <div class="card p-4">
        <p class="label">Total time</p>
        <p class="mt-1 font-display text-3xl font-bold">{{ formatSecs(stats.total_secs) }}</p>
      </div>
      <div class="card p-4">
        <p class="label">Projects</p>
        <p class="mt-1 font-display text-3xl font-bold text-accent">{{ stats.rows.length }}</p>
      </div>
    </div>

    <div class="card p-5">
      <p class="label mb-4">By project</p>
      <ul class="space-y-3">
        <li v-for="r in stats.rows" :key="r.key">
          <div class="mb-1 flex items-baseline justify-between text-sm">
            <span class="font-medium text-ink">{{ r.label }}</span>
            <span class="font-mono text-xs text-muted">{{ r.task_count }} tasks · {{ formatSecs(r.total_secs) }}</span>
          </div>
          <div class="h-2 overflow-hidden rounded-full bg-panel-2">
            <div class="h-full rounded-full bg-accent" :style="{ width: (r.total_secs / max) * 100 + '%' }" />
          </div>
        </li>
      </ul>
    </div>
  </section>
</template>
