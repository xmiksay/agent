<script setup lang="ts">
import { TRIGGER_TYPES } from "../util/triggerTypes";
import type { TriggerConfig } from "../types/api";

// Per-trigger-type gating editor. Bound via v-model to a fully-seeded map (one
// entry per TRIGGER_TYPES key — see seedTriggers). `enabled` shows for all
// types; mode/label only matter for "issue", so they render there alone.
const map = defineModel<Record<string, TriggerConfig>>({ required: true });
</script>

<template>
  <div>
    <span class="label">Per-type triggers</span>
    <p class="mb-2 text-xs text-muted">
      Override the service-level trigger default per task type. Disabling a type stops the agent
      reacting to it. Mode/label apply to issues only.
    </p>
    <div class="space-y-2">
      <div
        v-for="t in TRIGGER_TYPES"
        :key="t.value"
        class="flex flex-wrap items-center gap-3 rounded border border-line/60 p-2"
      >
        <label class="flex min-w-[9rem] items-center gap-2">
          <input v-model="map[t.value].enabled" type="checkbox" class="h-4 w-4" />
          <span class="text-sm text-ink">{{ t.label }}</span>
        </label>
        <template v-if="t.value === 'issue'">
          <select v-model="map[t.value].mode" class="select w-auto">
            <option value="assignee">Assignee</option>
            <option value="label">Label</option>
            <option value="both">Both</option>
          </select>
          <input
            v-if="map[t.value].mode !== 'assignee'"
            v-model="map[t.value].label"
            class="input w-auto flex-1 font-mono"
            placeholder="agent"
          />
        </template>
      </div>
    </div>
  </div>
</template>
