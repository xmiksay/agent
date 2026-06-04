<script setup lang="ts">
import { computed, onMounted } from "vue";
import { RouterLink, RouterView } from "vue-router";
import { useSessionStore } from "./stores/session";
import { probeAuth } from "./api/client";
import TokenGate from "./components/TokenGate.vue";

const session = useSessionStore();

onMounted(async () => {
  if (session.hasToken && session.validated === null) {
    const ok = await probeAuth();
    if (!ok) session.markInvalid();
  } else if (!session.hasToken) {
    // Optimistically probe once: if the server has no token configured,
    // /api/auth/check returns 204 and we stay unlocked.
    const ok = await probeAuth();
    if (ok) session.markValid();
    else session.markInvalid();
  }
});

const showGate = computed(() => session.validated === false);
</script>

<template>
  <div class="min-h-screen flex flex-col">
    <header class="bg-white border-b border-gray-200">
      <div class="max-w-6xl mx-auto px-4 py-3 flex gap-6 items-center">
        <span class="font-semibold tracking-tight">Claude Agent</span>
        <nav class="flex gap-4 text-sm">
          <RouterLink to="/" class="hover:text-blue-700">Tasks</RouterLink>
          <RouterLink to="/projects" class="hover:text-blue-700">Projects</RouterLink>
          <RouterLink to="/git_services" class="hover:text-blue-700">Services</RouterLink>
          <RouterLink to="/auth_requests" class="hover:text-blue-700">Auth queue</RouterLink>
        </nav>
        <button
          v-if="session.hasToken"
          class="ml-auto text-xs text-gray-500 hover:text-red-600"
          title="Sign out (clears local token)"
          @click="session.clear()"
        >
          sign out
        </button>
      </div>
    </header>
    <main class="flex-1 max-w-6xl mx-auto w-full px-4 py-6">
      <RouterView />
    </main>
    <TokenGate v-if="showGate" />
  </div>
</template>
