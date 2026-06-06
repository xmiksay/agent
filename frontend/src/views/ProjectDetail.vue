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
    <header class="flex items-center gap-3">
      <ProviderBadge :provider="store.detail.provider" />
      <h1 class="text-xl font-semibold">{{ store.detail.full_name }}</h1>
    </header>

    <dl class="grid grid-cols-2 gap-4 bg-white p-4 rounded shadow-sm text-sm">
      <div>
        <dt class="text-xs text-gray-500">My username</dt>
        <dd>{{ store.detail.my_username }}</dd>
      </div>
      <div>
        <dt class="text-xs text-gray-500">Default branch</dt>
        <dd>{{ store.detail.default_branch }}</dd>
      </div>
      <div class="col-span-2">
        <dt class="text-xs text-gray-500">Remote URL</dt>
        <dd class="font-mono text-xs">{{ store.detail.remote_url }}</dd>
      </div>
      <div class="col-span-2">
        <dt class="text-xs text-gray-500">Git service</dt>
        <dd v-if="store.detail.git_service_id">
          <RouterLink
            :to="`/git_services/${store.detail.git_service_id}`"
            class="text-blue-700 hover:underline font-mono text-xs"
          >
            {{ store.detail.git_service_id }}
          </RouterLink>
        </dd>
        <dd v-else class="text-xs text-gray-500">unlinked</dd>
      </div>
    </dl>

    <section class="bg-white p-4 rounded shadow-sm space-y-3">
      <h2 class="font-medium">Allowed operations</h2>
      <p class="text-xs text-gray-500">
        One glob per line. Patterns match the full command line. Anything not
        matched will pause the job and request operator approval.
      </p>
      <textarea
        v-model="draft"
        rows="10"
        class="w-full font-mono text-sm border rounded p-2"
      />
      <button
        class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        :disabled="saving"
        @click="save"
      >
        {{ saving ? "Saving…" : "Save" }}
      </button>
    </section>

    <section class="bg-white p-4 rounded shadow-sm space-y-3">
      <h2 class="font-medium">Environment variables</h2>
      <p class="text-xs text-gray-500">
        <code>KEY=value</code> per line, <code>.env</code> style. Unpacked into
        the agent's environment when it starts. Blank lines and <code>#</code>
        comments are ignored. The text is a
        <a
          href="https://docs.rs/minijinja/latest/minijinja/syntax/index.html"
          target="_blank"
          class="text-blue-700 hover:underline"
          >minijinja template</a
        >
        with the task's runtime variables:
        <code>branch</code>, <code>default_branch</code>, <code>url</code>,
        <code>project</code>, <code>service</code>, <code>task_id</code>.
      </p>
      <textarea
        v-model="envDraft"
        rows="8"
        :placeholder="envPlaceholder"
        class="w-full font-mono text-sm border rounded p-2"
      />
      <button
        class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        :disabled="savingEnv"
        @click="saveEnv"
      >
        {{ savingEnv ? "Saving…" : "Save" }}
      </button>
    </section>

    <section class="bg-white p-4 rounded shadow-sm">
      <h2 class="font-medium mb-2">Active branches</h2>
      <table class="min-w-full text-sm">
        <thead class="text-left text-xs uppercase text-gray-500 border-b">
          <tr>
            <th class="px-2 py-1">Branch</th>
            <th class="px-2 py-1">Status</th>
            <th class="px-2 py-1">Issue / PR</th>
            <th class="px-2 py-1">Last used</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="b in store.detail.branches" :key="b.id" class="border-b last:border-0">
            <td class="px-2 py-1 font-mono">{{ b.branch_name }}</td>
            <td class="px-2 py-1"><StatusPill :status="b.status" /></td>
            <td class="px-2 py-1">
              <span v-if="b.issue_iid">#{{ b.issue_iid }}</span>
              <span v-if="b.pr_iid">!{{ b.pr_iid }}</span>
            </td>
            <td class="px-2 py-1 text-xs text-gray-500">
              {{ new Date(b.last_used_at).toLocaleString() }}
            </td>
          </tr>
          <tr v-if="!store.detail.branches.length">
            <td colspan="4" class="px-2 py-3 text-center text-gray-500">
              No checked-out branches.
            </td>
          </tr>
        </tbody>
      </table>
    </section>
  </section>
  <p v-else class="text-gray-500">Loading…</p>
</template>
