<script setup lang="ts">
import { useRouter } from "vue-router";
import ProviderBadge from "../components/ProviderBadge.vue";
import { projects } from "../fixtures";

const router = useRouter();
</script>

<template>
  <section>
    <div class="mb-6">
      <h1 class="font-display text-2xl font-bold tracking-tight">Projects</h1>
      <p class="mt-1 text-sm text-muted">Repos the agent has discovered, with their guardrails.</p>
    </div>

    <div class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Provider</th><th>Project</th><th>Default branch</th>
            <th class="text-right">Branches</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in projects" :key="p.id" class="cursor-pointer" @click="router.push(`/projects/${p.id}`)">
            <td><ProviderBadge :provider="p.provider" /></td>
            <td class="font-medium text-ink">{{ p.full_name }}</td>
            <td class="font-mono text-xs text-muted">{{ p.default_branch }}</td>
            <td class="text-right font-mono text-xs text-muted">{{ p.branch_count }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
