<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useProjectsStore } from "../stores/projects";
import ProviderBadge from "../components/ProviderBadge.vue";

const store = useProjectsStore();
const router = useRouter();

function open(id: string) {
  router.push(`/projects/${id}`);
}

onMounted(() => store.refresh());
</script>

<template>
  <section>
    <div class="mb-6">
      <h1 class="font-display text-2xl font-bold tracking-tight">Projects</h1>
      <p class="mt-1 text-sm text-muted">Repos the agent has discovered, with their guardrails.</p>
    </div>

    <div v-if="store.loading" class="text-muted">Loading…</div>
    <div v-else-if="!store.list.length" class="card p-10 text-center text-faint">
      No projects yet — they appear after the first webhook event.
    </div>
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Provider</th>
            <th>Project</th>
            <th>Default branch</th>
            <th class="text-right">Branches</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in store.list" :key="p.id" class="cursor-pointer" @click="open(p.id)">
            <td><ProviderBadge :provider="p.provider" /></td>
            <td>
              <RouterLink
                :to="`/projects/${p.id}`"
                class="font-medium text-ink hover:text-accent"
                @click.stop
              >
                {{ p.full_name }}
              </RouterLink>
            </td>
            <td class="font-mono text-xs text-muted">{{ p.default_branch }}</td>
            <td class="text-right font-mono text-xs text-muted">{{ p.branch_count }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
