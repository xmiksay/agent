<script setup lang="ts">
import { ref } from "vue";
import { RouterLink, RouterView } from "vue-router";
import Logo from "./components/Logo.vue";
import Background from "./components/Background.vue";
import Switcher from "./components/Switcher.vue";
import { layout } from "./layout";
import { authRequests } from "./fixtures";

// Workbench shell. The live App.vue wires session + the shared WebSocket; here
// the chrome is the design artifact and the pending-approval dot reads fixtures.
const nav = [
  { to: "/", label: "Tasks", icon: "▦" },
  { to: "/projects", label: "Projects", icon: "◇" },
  { to: "/git_services", label: "Services", icon: "⬡" },
  { to: "/auth_requests", label: "Auth queue", icon: "⚿" },
  { to: "/stats", label: "Stats", icon: "▤" },
  { to: "/components", label: "Components", icon: "✦" },
];
const pending = ref(authRequests.filter((a) => a.status === "pending").length);

// Under `md` both desktop chromes collapse to one top bar with a drawer.
const menuOpen = ref(false);
</script>

<template>
  <div class="relative isolate" :class="layout === 'side' ? 'flex min-h-screen' : 'min-h-screen'">
    <Background />

    <!-- ===== Sidebar layout ===== -->
    <aside
      v-if="layout === 'side'"
      class="sticky top-0 hidden h-screen w-56 shrink-0 flex-col border-r border-line bg-panel/40 md:flex"
    >
      <div class="flex items-center gap-2.5 px-4 py-4">
        <Logo :size="30" />
        <span class="font-display text-lg font-bold tracking-tight">Agent</span>
        <span class="led led-pulse ml-auto text-signal-live" title="live" />
      </div>
      <nav class="flex flex-1 flex-col gap-0.5 px-2 py-2">
        <RouterLink
          v-for="n in nav"
          :key="n.to"
          :to="n.to"
          class="group flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted transition-colors hover:bg-panel-2 hover:text-ink"
          active-class="!bg-panel-2 !text-ink"
        >
          <span class="w-4 text-center text-faint group-hover:text-accent">{{ n.icon }}</span>
          {{ n.label }}
          <span v-if="n.label === 'Auth queue' && pending" class="led ml-auto text-accent" :title="`${pending} pending`" />
        </RouterLink>
      </nav>
      <div class="border-t border-line px-4 py-3 font-mono text-[11px] text-faint">
        <span class="led mr-1.5 text-signal-ok" /> ~/projects/agent
      </div>
    </aside>

    <!-- ===== Top-menu layout (desktop) ===== -->
    <header
      v-else
      class="sticky top-0 z-30 hidden border-b border-line bg-canvas/80 backdrop-blur-md md:block"
    >
      <div class="mx-auto flex max-w-6xl items-center gap-6 px-8 py-3">
        <div class="flex items-center gap-2.5">
          <Logo :size="28" />
          <span class="font-display text-lg font-bold tracking-tight">Agent</span>
        </div>
        <nav class="flex items-center gap-1">
          <RouterLink
            v-for="n in nav"
            :key="n.to"
            :to="n.to"
            class="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium text-muted transition-colors hover:bg-panel-2 hover:text-ink"
            active-class="!bg-panel-2 !text-ink"
          >
            {{ n.label }}
            <span v-if="n.label === 'Auth queue' && pending" class="led text-accent" :title="`${pending} pending`" />
          </RouterLink>
        </nav>
        <div class="ml-auto flex items-center gap-3">
          <code class="hidden rounded border border-line bg-panel px-2 py-1 font-mono text-xs text-muted lg:inline">~/projects/agent</code>
          <span class="led led-pulse text-signal-live" title="live" />
        </div>
      </div>
    </header>

    <!-- ===== Content ===== -->
    <div class="flex min-w-0 flex-1 flex-col">
      <!-- Mobile top bar — both desktop chromes collapse to this under md -->
      <header class="sticky top-0 z-40 border-b border-line bg-canvas/85 backdrop-blur-md md:hidden">
        <div class="flex items-center gap-2.5 px-4 py-3">
          <Logo :size="26" />
          <span class="font-display text-base font-bold tracking-tight">Agent</span>
          <span class="led led-pulse text-signal-live" title="live" />
          <button
            class="relative -mr-1 ml-auto rounded-md border border-line-2 px-2.5 py-1.5 text-muted transition-colors hover:text-ink"
            :aria-expanded="menuOpen"
            aria-label="Menu"
            @click="menuOpen = !menuOpen"
          >
            <span class="text-base leading-none">{{ menuOpen ? "✕" : "☰" }}</span>
            <span v-if="pending && !menuOpen" class="led absolute -right-1 -top-1 text-accent" />
          </button>
        </div>
        <nav v-if="menuOpen" class="flex flex-col gap-0.5 border-t border-line px-2 py-2">
          <RouterLink
            v-for="n in nav"
            :key="n.to"
            :to="n.to"
            class="group flex items-center gap-3 rounded-md px-3 py-2.5 text-sm font-medium text-muted transition-colors hover:bg-panel-2 hover:text-ink"
            active-class="!bg-panel-2 !text-ink"
            @click="menuOpen = false"
          >
            <span class="w-4 text-center text-faint group-hover:text-accent">{{ n.icon }}</span>
            {{ n.label }}
            <span v-if="n.label === 'Auth queue' && pending" class="led ml-auto text-accent" :title="`${pending} pending`" />
          </RouterLink>
        </nav>
      </header>

      <main class="mx-auto w-full max-w-6xl flex-1 px-4 py-6 sm:px-8 sm:py-8">
        <RouterView />
      </main>
    </div>

    <!-- ===== Combined layout + theme switch ===== -->
    <Switcher />
  </div>
</template>
