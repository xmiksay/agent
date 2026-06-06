<script setup lang="ts">
import { ref } from "vue";
import { RouterLink } from "vue-router";
import ProviderBadge from "../components/ProviderBadge.vue";
import StatusPill from "../components/StatusPill.vue";
import { branchesByProject, projects, variablesByProject } from "../fixtures";

const props = defineProps<{ id: string }>();
const detail = projects.find((p) => p.id === props.id);
const branches = branchesByProject[props.id] ?? [];
const draft = ref(detail?.allowed_operations.join("\n") ?? "");

// Project variables, edited as a .env file — one KEY=value per line. Seeded from
// the stored vars; the live view would PUT the parsed result to the config endpoint.
const envText = ref(
  (variablesByProject[props.id] ?? []).map((v) => `${v.key}=${v.value}`).join("\n"),
);
const savedVars = ref(false);
function saveVars() {
  savedVars.value = true;
  setTimeout(() => (savedVars.value = false), 1500);
}
</script>

<template>
  <section v-if="detail" class="space-y-6">
    <RouterLink to="/projects" class="inline-block text-sm text-muted hover:text-accent">← Projects</RouterLink>

    <header class="flex items-center gap-3">
      <ProviderBadge :provider="detail.provider" />
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ detail.full_name }}</h1>
      <span class="tag ml-auto">{{ branches.length }} branches</span>
    </header>

    <dl class="card grid grid-cols-2 gap-4 p-5 text-sm">
      <div><dt class="label mb-0.5">Default branch</dt><dd class="font-mono text-muted">{{ detail.default_branch }}</dd></div>
      <div><dt class="label mb-0.5">Slug</dt><dd class="font-mono text-muted">{{ detail.project_slug }}</dd></div>
      <div class="col-span-2"><dt class="label mb-0.5">Remote URL</dt><dd class="break-all font-mono text-xs text-muted">{{ detail.remote_url }}</dd></div>
    </dl>

    <section class="card space-y-3 p-5">
      <div class="flex items-center gap-2">
        <h2 class="text-sm font-semibold">Variables</h2>
        <span class="tag">.env</span>
        <span class="ml-auto text-xs text-faint">injected into the agent's worktree environment</span>
      </div>
      <p class="text-xs text-muted">One <code class="text-ink">KEY=value</code> per line. <code class="text-ink">#</code> starts a comment.</p>
      <textarea
        v-model="envText"
        rows="8"
        spellcheck="false"
        class="textarea font-mono text-xs leading-relaxed"
        placeholder="NODE_VERSION=20&#10;GH_TOKEN=…"
      />
      <div class="flex">
        <button class="btn btn-primary btn-sm ml-auto" @click="saveVars">
          {{ savedVars ? "Saved ✓" : "Save variables" }}
        </button>
      </div>
    </section>

    <section class="card space-y-3 p-5">
      <h2 class="text-sm font-semibold">Allowed operations</h2>
      <p class="text-xs text-muted">
        One glob per line. Patterns match the full command line; anything not
        matched pauses the job and requests operator approval.
      </p>
      <textarea v-model="draft" rows="8" class="textarea font-mono text-xs" />
      <button class="btn btn-primary btn-sm">Save</button>
    </section>

    <section class="card overflow-hidden">
      <h2 class="border-b border-line px-4 py-3 text-sm font-semibold">Active branches</h2>
      <div class="overflow-x-auto">
      <table class="tbl">
        <thead><tr><th>Branch</th><th>Status</th><th>Issue / PR</th><th class="text-right">Last used</th></tr></thead>
        <tbody>
          <tr v-for="b in branches" :key="b.id">
            <td class="font-mono text-xs text-ink">{{ b.branch_name }}</td>
            <td><StatusPill :status="b.status" /></td>
            <td class="font-mono text-xs text-muted">
              <span v-if="b.issue_iid">#{{ b.issue_iid }}</span>
              <span v-if="b.pr_iid">!{{ b.pr_iid }}</span>
              <span v-if="!b.issue_iid && !b.pr_iid">—</span>
            </td>
            <td class="text-right text-xs text-faint">{{ new Date(b.last_used_at).toLocaleString() }}</td>
          </tr>
          <tr v-if="!branches.length"><td colspan="4" class="py-6 text-center text-faint">No checked-out branches.</td></tr>
        </tbody>
      </table>
      </div>
    </section>
  </section>
  <p v-else class="text-faint">Project not found.</p>
</template>
