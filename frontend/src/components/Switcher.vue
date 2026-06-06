<script setup lang="ts">
import { layout } from "../layout";
import { theme } from "../theme";

// Inline Layout (Top/Side) + Theme (Dark/Light) toggles, embedded in the app
// chrome. `compact` hides the word labels for narrow hosts (sidebar/drawer).
// State lives in src/layout.ts and src/theme.ts.
defineProps<{ compact?: boolean }>();
</script>

<template>
  <div
    class="inline-flex flex-wrap items-center gap-1 rounded-full border border-line-2 bg-panel/60 p-1"
  >
    <!-- Layout toggle only affects the desktop chrome, so it's md-and-up only. -->
    <span
      v-if="!compact"
      class="hidden pl-2 font-mono text-[10px] uppercase tracking-label text-faint md:inline"
      >layout</span
    >
    <div class="hidden items-center gap-1 md:flex">
      <button
        class="rounded-full px-2.5 py-1 text-xs font-semibold transition-colors"
        :class="layout === 'top' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="layout = 'top'"
      >Top</button>
      <button
        class="rounded-full px-2.5 py-1 text-xs font-semibold transition-colors"
        :class="layout === 'side' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="layout = 'side'"
      >Side</button>
    </div>

    <span class="mx-0.5 hidden h-4 w-px bg-line-2 md:inline-block" />

    <span
      v-if="!compact"
      class="font-mono text-[10px] uppercase tracking-label text-faint"
      >theme</span
    >
    <div class="flex items-center gap-1 pr-0.5">
      <button
        class="rounded-full px-2.5 py-1 text-xs font-semibold transition-colors"
        :class="theme === 'dark' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="theme = 'dark'"
      >Dark</button>
      <button
        class="rounded-full px-2.5 py-1 text-xs font-semibold transition-colors"
        :class="theme === 'light' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="theme = 'light'"
      >Light</button>
    </div>
  </div>
</template>
