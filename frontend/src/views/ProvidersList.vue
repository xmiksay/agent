<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useProvidersStore } from "../stores/providers";
import type { NewProvider } from "../types/api";

const store = useProvidersStore();
const router = useRouter();

function open(id: string) {
  router.push(`/providers/${id}`);
}

const showForm = ref(false);
const form = ref<NewProvider>(blank());
const saving = ref(false);
const error = ref<string | null>(null);

function blank(): NewProvider {
  return {
    kind: store.kinds[0] ?? "claude_code",
    name: "",
    api_key: "",
    api_url: "",
  };
}

async function submit() {
  saving.value = true;
  error.value = null;
  try {
    const body: NewProvider = { kind: form.value.kind, name: form.value.name };
    if (form.value.api_key) body.api_key = form.value.api_key;
    if (form.value.api_url) body.api_url = form.value.api_url;
    await store.create(body);
    form.value = blank();
    showForm.value = false;
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove(id: string, name: string) {
  if (!confirm(`Delete provider "${name}"?`)) return;
  try {
    await store.remove(id);
  } catch (e: unknown) {
    alert(extractMessage(e));
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

onMounted(async () => {
  await store.refresh();
  if (!form.value.kind) form.value.kind = store.kinds[0] ?? "claude_code";
});
</script>

<template>
  <section class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Providers</h1>
        <p class="mt-1 text-sm text-muted">
          Model providers — the backend plus an optional API key that models reference.
        </p>
      </div>
      <button v-if="!showForm" class="btn btn-primary" @click="showForm = true">+ Add provider</button>
    </div>

    <form v-if="showForm" class="card space-y-3 p-5" @submit.prevent="submit">
      <div class="grid grid-cols-2 gap-3">
        <label class="flex flex-col">
          <span class="label">Kind</span>
          <select v-model="form.kind" class="select">
            <option v-for="k in store.kinds" :key="k" :value="k">{{ k }}</option>
          </select>
        </label>
        <label class="flex flex-col">
          <span class="label">Name (display name)</span>
          <input v-model="form.name" required placeholder="Anthropic" class="input" />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">API key <span class="text-faint">(optional, write-only)</span></span>
          <input v-model="form.api_key" type="password" autocomplete="off" class="input font-mono" />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">API base URL <span class="text-faint">(optional)</span></span>
          <input
            v-model="form.api_url"
            placeholder="http://localhost:11434"
            class="input font-mono"
          />
          <span class="mt-1 text-xs text-faint">
            Override the provider endpoint — e.g. Ollama: http://localhost:11434. Leave blank for
            the default.
          </span>
        </label>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <div class="flex gap-2">
        <button type="submit" :disabled="saving" class="btn btn-primary">
          {{ saving ? "Saving…" : "Create" }}
        </button>
        <button type="button" class="btn btn-ghost" @click="showForm = false">Cancel</button>
      </div>
    </form>

    <div v-if="store.loading" class="text-muted">Loading…</div>
    <div v-else-if="!store.list.length" class="card p-10 text-center text-faint">
      No providers configured. Add one before creating models.
    </div>
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Name</th>
            <th>Kind</th>
            <th>API key</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in store.list" :key="p.id" class="cursor-pointer" @click="open(p.id)">
            <td>
              <RouterLink
                :to="`/providers/${p.id}`"
                class="font-medium text-ink hover:text-accent"
                @click.stop
              >
                {{ p.name }}
              </RouterLink>
            </td>
            <td class="font-mono text-xs text-faint">{{ p.kind }}</td>
            <td>
              <span v-if="p.has_api_key" class="pill bg-signal-ok/15 text-signal-ok">API key set</span>
              <span v-else class="pill bg-panel-2 text-faint">none</span>
            </td>
            <td class="text-right">
              <button
                class="text-xs text-signal-danger hover:underline"
                @click.stop="remove(p.id, p.name)"
              >
                delete
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
