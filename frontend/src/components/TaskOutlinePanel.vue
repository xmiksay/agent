<script setup lang="ts">
// Outline — background-task / sub-agent completions, lifted out of the inline
// timeline so they don't read as noise.
import Accordion from "./Accordion.vue";
import type { TaskNotification } from "../composables/useClaudeStream";

defineProps<{ notifications: TaskNotification[] }>();

const open = defineModel<boolean>("open", { required: true });
</script>

<template>
  <Accordion
    v-model:open="open"
    title="Outline"
    :subtitle="`${notifications.length} task${notifications.length === 1 ? '' : 's'}`"
  >
    <ul class="space-y-2 pt-3">
      <li
        v-for="(n, i) in notifications"
        :key="i"
        class="space-y-1 rounded-md border border-line bg-panel-2/60 p-3"
      >
        <div class="flex items-center gap-2">
          <span
            class="rounded px-1.5 py-0.5 text-[10px] uppercase tracking-label"
            :class="
              n.status === 'completed'
                ? 'bg-signal-ok/15 text-signal-ok'
                : n.status === 'failed' || /error/i.test(n.status)
                  ? 'bg-signal-danger/15 text-signal-danger'
                  : 'bg-panel text-faint'
            "
          >
            {{ n.status || "task" }}
          </span>
          <span v-if="n.bgTaskId" class="font-mono text-[11px] text-faint">{{ n.bgTaskId }}</span>
        </div>
        <details>
          <summary class="cursor-pointer">
            <span class="truncate font-mono text-[11px] text-muted">{{ n.summary }}</span>
          </summary>
          <pre class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap font-mono text-[11px] text-muted">{{ n.summary }}</pre>
        </details>
        <div v-if="n.outputFile" class="font-mono text-[11px] text-faint">→ {{ n.outputFile }}</div>
      </li>
    </ul>
  </Accordion>
</template>
