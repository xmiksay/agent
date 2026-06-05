<script setup lang="ts">
// Collapsible section. `open` is a v-model so the parent can drive it (auto-open
// on incoming events, lazy-load body on first expand). Header carries an optional
// subtitle and an `actions` slot whose clicks don't toggle the panel.
defineProps<{ title: string; subtitle?: string }>();
const open = defineModel<boolean>("open", { default: false });
</script>

<template>
  <section class="bg-white rounded shadow-sm overflow-hidden">
    <button
      class="w-full flex items-baseline gap-2 px-4 py-2.5 text-left hover:bg-gray-50"
      @click="open = !open"
    >
      <span class="text-gray-400 text-xs w-3 shrink-0">{{ open ? "▾" : "▸" }}</span>
      <span class="text-sm font-medium">{{ title }}</span>
      <span v-if="subtitle" class="text-xs text-gray-500 truncate">{{ subtitle }}</span>
      <span class="ml-auto flex items-center gap-2 shrink-0" @click.stop>
        <slot name="actions" />
      </span>
    </button>
    <div v-if="open" class="px-4 pb-4 border-t border-gray-100">
      <slot />
    </div>
  </section>
</template>
