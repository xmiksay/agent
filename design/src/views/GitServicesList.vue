<script setup lang="ts">
import { useRouter } from "vue-router";
import ProviderBadge from "../components/ProviderBadge.vue";
import { services } from "../fixtures";

const router = useRouter();
</script>

<template>
  <section>
    <div class="mb-6 flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Git services</h1>
        <p class="mt-1 text-sm text-muted">Provider connections — tokens and webhook secrets stay write-only.</p>
      </div>
      <button class="btn btn-primary">+ Add service</button>
    </div>

    <div class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr><th>Kind</th><th>Name</th><th>Base URL</th><th>Bot user</th><th>Webhook</th></tr>
        </thead>
        <tbody>
          <tr v-for="s in services" :key="s.id" class="cursor-pointer" @click="router.push(`/git_services/${s.id}`)">
            <td><ProviderBadge :provider="s.kind" /></td>
            <td class="font-medium text-ink">{{ s.display_name }}</td>
            <td class="font-mono text-xs text-muted">{{ s.base_url }}</td>
            <td class="font-mono text-xs text-muted">{{ s.bot_username }}</td>
            <td class="font-mono text-xs text-faint">{{ s.webhook_path }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
