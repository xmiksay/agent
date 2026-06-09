<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useProjectsStore } from "../stores/projects";
import { useServicesStore } from "../stores/services";
import ProviderBadge from "../components/ProviderBadge.vue";

const store = useProjectsStore();
const services = useServicesStore();
const router = useRouter();

const showForm = ref(false);
const form = ref(blank());
const saving = ref(false);
const error = ref<string | null>(null);

function blank() {
  return {
    service_id: "",
    full_name: "",
    default_branch: "",
    remote_url: "",
  };
}

function open(id: string) {
  router.push(`/projects/${id}`);
}

async function submit() {
  saving.value = true;
  error.value = null;
  try {
    const created = await store.create({
      service_id: form.value.service_id,
      full_name: form.value.full_name.trim(),
      default_branch: form.value.default_branch.trim() || undefined,
      remote_url: form.value.remote_url.trim() || undefined,
    });
    form.value = blank();
    showForm.value = false;
    router.push(`/projects/${created.id}`);
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

function extractMessage(e: unknown): string {
  if (typeof e === "object" && e !== null) {
    const err = e as { data?: unknown; message?: string };
    if (typeof err.data === "string") return err.data;
    if (err.message) return err.message;
  }
  return String(e);
}

onMounted(() => {
  store.refresh();
  services.refresh();
});
</script>

<template>
  <section class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Projects</h1>
        <p class="mt-1 text-sm text-muted">Repos the agent has discovered, with their guardrails.</p>
      </div>
      <button v-if="!showForm" class="btn btn-primary" @click="showForm = true">+ Add project</button>
    </div>

    <form v-if="showForm" class="card space-y-3 p-5" @submit.prevent="submit">
      <div class="grid grid-cols-2 gap-3">
        <label class="col-span-2 flex flex-col">
          <span class="label">Service</span>
          <select v-model="form.service_id" required class="select">
            <option value="" disabled>Select a service…</option>
            <option v-for="s in services.list" :key="s.id" :value="s.id">
              {{ s.display_name }} ({{ s.kind }})
            </option>
          </select>
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">Full name</span>
          <input v-model="form.full_name" required placeholder="owner/repo" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Default branch</span>
          <input v-model="form.default_branch" placeholder="main" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Remote URL <span class="text-faint">(optional)</span></span>
          <input v-model="form.remote_url" class="input font-mono" />
          <span class="mt-1 text-xs text-muted">
            Leave blank to derive from the service; accepts SSH or HTTPS.
          </span>
        </label>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <div class="flex gap-2">
        <button
          type="submit"
          :disabled="saving || !form.service_id || !form.full_name.trim()"
          class="btn btn-primary"
        >
          {{ saving ? "Saving…" : "Create" }}
        </button>
        <button type="button" class="btn btn-ghost" @click="showForm = false">Cancel</button>
      </div>
    </form>

    <div v-if="store.loading" class="text-muted">Loading…</div>
    <div v-else-if="!store.list.length" class="card p-10 text-center text-faint">
      No projects yet — they appear after the first webhook event, or add one above.
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
