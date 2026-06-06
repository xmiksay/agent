<script setup lang="ts">
// Collapsible section. `open` is a v-model so the parent can drive it (auto-open
// on incoming events, lazy-load body on first expand). Header carries an optional
// subtitle and an `actions` slot whose clicks don't toggle the panel.
defineProps<{ title: string; subtitle?: string }>();
const open = defineModel<boolean>("open", { default: false });
</script>

<template>
  <section class="card overflow-hidden">
    <button
      class="flex w-full items-baseline gap-2 px-4 py-2.5 text-left transition-colors hover:bg-panel-2/70"
      @click="open = !open"
    >
      <span class="w-3 shrink-0 text-xs text-faint transition-transform" :class="{ 'rotate-90': open }">▸</span>
      <span class="text-sm font-medium text-ink">{{ title }}</span>
      <span v-if="subtitle" class="truncate text-xs text-muted">{{ subtitle }}</span>
      <span class="ml-auto flex shrink-0 items-center gap-2" @click.stop>
        <slot name="actions" />
      </span>
    </button>
    <div v-if="open" class="border-t border-line px-4 pb-4 pt-3">
      <slot />
    </div>
  </section>
</template>
