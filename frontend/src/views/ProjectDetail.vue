<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { useProjectsStore } from "../stores/projects";
import ProviderBadge from "../components/ProviderBadge.vue";
import StatusPill from "../components/StatusPill.vue";

const props = defineProps<{ id: string }>();
const store = useProjectsStore();
const draft = ref("");
const saving = ref(false);
const envDraft = ref("");
const savingEnv = ref(false);
const envPlaceholder = "DEPLOY_ENV={{ branch }}\nREPO_URL={{ url }}";

const reload = async () => {
  await store.load(props.id);
  draft.value = (store.detail?.allowed_operations ?? []).join("\n");
  envDraft.value = store.detail?.env_file ?? "";
};

onMounted(reload);
watch(() => props.id, reload);

async function save() {
  const ops = draft.value
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  saving.value = true;
  try {
    await store.updateAllowedOps(props.id, ops);
  } finally {
    saving.value = false;
  }
}

async function saveEnv() {
  savingEnv.value = true;
  try {
    await store.updateEnvFile(props.id, envDraft.value);
  } finally {
    savingEnv.value = false;
  }
}
</script>

<template>
  <section v-if="store.detail" class="space-y-6">
    <RouterLink to="/projects" class="inline-block text-sm text-muted hover:text-accent">← Projects</RouterLink>

    <header class="flex items-center gap-3">
      <ProviderBadge :provider="store.detail.provider" />
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ store.detail.full_name }}</h1>
      <span class="tag ml-auto">{{ store.detail.branches.length }} branches</span>
    </header>

    <dl class="card grid grid-cols-2 gap-4 p-5 text-sm">
      <div>
        <dt class="label mb-0.5">My username</dt>
        <dd class="text-muted">{{ store.detail.my_username }}</dd>
      </div>
      <div>
        <dt class="label mb-0.5">Default branch</dt>
        <dd class="font-mono text-muted">{{ store.detail.default_branch }}</dd>
      </div>
      <div class="col-span-2">
        <dt class="label mb-0.5">Remote URL</dt>
        <dd class="break-all font-mono text-xs text-muted">{{ store.detail.remote_url }}</dd>
      </div>
      <div class="col-span-2">
        <dt class="label mb-0.5">Git service</dt>
        <dd v-if="store.detail.git_service_id">
          <RouterLink
            :to="`/git_services/${store.detail.git_service_id}`"
            class="font-mono text-xs text-accent hover:underline"
          >
            {{ store.detail.git_service_id }}
          </RouterLink>
        </dd>
        <dd v-else class="text-xs text-faint">unlinked</dd>
      </div>
    </dl>

    <section class="card space-y-3 p-5">
      <h2 class="text-sm font-semibold">Allowed operations</h2>
      <p class="text-xs text-muted">
        One glob per line. Patterns match the full command line. Anything not
        matched will pause the job and request operator approval.
      </p>
      <textarea v-model="draft" rows="10" class="textarea font-mono text-xs" />
      <button class="btn btn-primary btn-sm" :disabled="saving" @click="save">
        {{ saving ? "Saving…" : "Save" }}
      </button>
    </section>

    <section class="card space-y-3 p-5">
      <h2 class="text-sm font-semibold">Environment variables</h2>
      <p class="text-xs text-muted">
        <code class="text-ink">KEY=value</code> per line, <code class="text-ink">.env</code> style. Unpacked into
        the agent's environment when it starts. Blank lines and <code class="text-ink">#</code>
        comments are ignored. The text is a
        <a
          href="https://docs.rs/minijinja/latest/minijinja/syntax/index.html"
          target="_blank"
          >minijinja template</a
        >
        with the task's runtime variables:
        <code class="text-ink">branch</code>, <code class="text-ink">default_branch</code>, <code class="text-ink">url</code>,
        <code class="text-ink">project</code>, <code class="text-ink">service</code>, <code class="text-ink">task_id</code>.
      </p>
      <textarea
        v-model="envDraft"
        rows="8"
        :placeholder="envPlaceholder"
        class="textarea font-mono text-xs"
      />
      <button class="btn btn-primary btn-sm" :disabled="savingEnv" @click="saveEnv">
        {{ savingEnv ? "Saving…" : "Save" }}
      </button>
    </section>

    <section class="card overflow-x-auto">
      <h2 class="border-b border-line px-4 py-3 text-sm font-semibold">Active branches</h2>
      <table class="tbl">
        <thead>
          <tr>
            <th>Branch</th>
            <th>Status</th>
            <th>Issue / PR</th>
            <th class="text-right">Last used</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="b in store.detail.branches" :key="b.id">
            <td class="font-mono text-xs text-ink">{{ b.branch_name }}</td>
            <td><StatusPill :status="b.status" /></td>
            <td class="font-mono text-xs text-muted">
              <span v-if="b.issue_iid">#{{ b.issue_iid }}</span>
              <span v-if="b.pr_iid">!{{ b.pr_iid }}</span>
              <span v-if="!b.issue_iid && !b.pr_iid">—</span>
            </td>
            <td class="text-right text-xs text-faint">
              {{ new Date(b.last_used_at).toLocaleString() }}
            </td>
          </tr>
          <tr v-if="!store.detail.branches.length">
            <td colspan="4" class="py-6 text-center text-faint">No checked-out branches.</td>
          </tr>
        </tbody>
      </table>
    </section>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
