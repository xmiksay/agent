<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useProvidersStore } from "../stores/providers";
import type { UpdateProvider } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useProvidersStore();
const router = useRouter();

// api_url is not secret, so it is prefilled and edited directly; "" round-trips
// as a clear (null) on save.
const draft = ref<{ kind: string; name: string; api_url: string }>({
  kind: "",
  name: "",
  api_url: "",
});
// New API key to set. Blank leaves the stored key untouched unless `clearKey`
// is checked, which sends null to clear it.
const apiKey = ref("");
const clearKey = ref(false);
const saving = ref(false);
const error = ref<string | null>(null);

const detail = computed(() => store.detail);

async function reload() {
  await store.refresh();
  await store.load(props.id);
  if (store.detail) {
    draft.value = {
      kind: store.detail.kind,
      name: store.detail.name,
      api_url: store.detail.api_url ?? "",
    };
    apiKey.value = "";
    clearKey.value = false;
  }
}

onMounted(reload);
watch(() => props.id, reload);

async function save() {
  saving.value = true;
  error.value = null;
  try {
    const body: UpdateProvider = {
      kind: draft.value.kind,
      name: draft.value.name,
      // "" clears the override; a string sets it.
      api_url: draft.value.api_url || null,
    };
    if (clearKey.value) body.api_key = null;
    else if (apiKey.value) body.api_key = apiKey.value;
    await store.update(props.id, body);
    apiKey.value = "";
    clearKey.value = false;
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove() {
  if (!detail.value) return;
  if (!confirm(`Delete provider "${detail.value.name}"?`)) return;
  try {
    await store.remove(props.id);
    router.push({ name: "providers" });
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
</script>

<template>
  <section v-if="detail" class="space-y-6">
    <RouterLink to="/providers" class="inline-block text-sm text-muted hover:text-accent"
      >← Providers</RouterLink
    >

    <header class="flex items-center gap-3">
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ detail.name }}</h1>
      <span class="font-mono text-sm text-faint">{{ detail.kind }}</span>
      <span v-if="detail.has_api_key" class="pill bg-signal-ok/15 text-signal-ok">API key set</span>
      <button class="btn btn-danger btn-sm ml-auto" @click="remove">Delete provider</button>
    </header>

    <form class="card space-y-3 p-5" @submit.prevent="save">
      <h2 class="text-sm font-semibold">Settings</h2>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label">Kind</label>
          <select v-model="draft.kind" class="select">
            <option v-for="k in store.kinds" :key="k" :value="k">{{ k }}</option>
          </select>
        </div>
        <div>
          <label class="label">Name</label>
          <input v-model="draft.name" class="input" />
        </div>
        <div class="col-span-2">
          <label class="label">
            API key
            <span class="text-faint">(write-only — blank keeps the stored key)</span>
          </label>
          <input
            v-model="apiKey"
            type="password"
            autocomplete="off"
            :disabled="clearKey"
            :placeholder="detail.has_api_key ? '•••••••• (unchanged)' : 'not set'"
            class="input font-mono"
          />
          <label v-if="detail.has_api_key" class="mt-2 flex items-center gap-2">
            <input v-model="clearKey" type="checkbox" class="h-4 w-4" />
            <span class="text-sm text-ink">Clear the stored API key</span>
          </label>
        </div>
        <div class="col-span-2">
          <label class="label">API base URL <span class="text-faint">(optional)</span></label>
          <input
            v-model="draft.api_url"
            placeholder="http://localhost:11434"
            class="input font-mono"
          />
          <span class="mt-1 block text-xs text-faint">
            Override the provider endpoint — e.g. Ollama: http://localhost:11434. Leave blank for
            the default.
          </span>
        </div>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <div class="flex gap-2">
        <button type="submit" :disabled="saving" class="btn btn-primary">
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <RouterLink :to="{ name: 'providers' }" class="btn btn-ghost">Back</RouterLink>
      </div>
    </form>

    <p class="font-mono text-xs text-faint">
      Updated {{ new Date(detail.updated_at).toLocaleString() }} · created
      {{ new Date(detail.created_at).toLocaleString() }}
    </p>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
