<script setup lang="ts">
import { layout } from "../layout";
import { theme } from "../theme";

// One bottom-right control: the Layout (Top/Side) and Theme (Dark/Light) toggles
// in a single pill. State lives in src/layout.ts and src/theme.ts.
</script>

<template>
  <div class="fixed bottom-4 right-4 z-50 flex items-center gap-2 rounded-full border border-line-2 bg-panel/90 p-1 shadow-[0_8px_30px_-10px_rgba(0,0,0,0.8)] backdrop-blur-md">
    <!-- Layout toggle only affects the desktop chrome, so it's md-and-up only. -->
    <span class="hidden pl-2 font-mono text-[10px] uppercase tracking-label text-faint md:inline">layout</span>
    <div class="hidden items-center gap-1 md:flex">
      <button
        class="rounded-full px-3 py-1 text-xs font-semibold transition-colors"
        :class="layout === 'top' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="layout = 'top'"
      >Top</button>
      <button
        class="rounded-full px-3 py-1 text-xs font-semibold transition-colors"
        :class="layout === 'side' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="layout = 'side'"
      >Side</button>
    </div>

    <span class="ml-1 hidden h-4 w-px bg-line-2 md:inline-block" />

    <span class="font-mono text-[10px] uppercase tracking-label text-faint">theme</span>
    <div class="flex items-center gap-1 pr-1">
      <button
        class="rounded-full px-3 py-1 text-xs font-semibold transition-colors"
        :class="theme === 'dark' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="theme = 'dark'"
      >Dark</button>
      <button
        class="rounded-full px-3 py-1 text-xs font-semibold transition-colors"
        :class="theme === 'light' ? 'bg-accent text-accent-ink' : 'text-muted hover:text-ink'"
        @click="theme = 'light'"
      >Light</button>
    </div>
  </div>
</template>
