<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useModelsStore } from "../stores/models";
import { useModelProvidersStore } from "../stores/model_providers";
import type { ModelEffort, NewModel } from "../types/api";

const store = useModelsStore();
const providers = useModelProvidersStore();
const router = useRouter();

// Resolve a provider id to its display name for the table.
const providerName = computed(() => {
  const byId = new Map(providers.list.map((p) => [p.id, p.name]));
  return (id: string) => byId.get(id) ?? id;
});

function open(id: string) {
  router.push(`/models/${id}`);
}

const showForm = ref(false);
const form = ref<NewModel>(blank());
// Effort kept as a plain string so the empty option maps cleanly to null.
const effort = ref<"" | ModelEffort>("");
const saving = ref(false);
const error = ref<string | null>(null);

function blank(): NewModel {
  return {
    provider_id: providers.options[0]?.value ?? "",
    model_id: "",
    alias: "",
    input_price: 0,
    output_price: 0,
    cache_write_price: 0,
    cache_read_price: 0,
    thinking: false,
    is_default: false,
  };
}

async function submit() {
  saving.value = true;
  error.value = null;
  try {
    const body: NewModel = { ...form.value, effort: effort.value || null };
    await store.create(body);
    form.value = blank();
    effort.value = "";
    showForm.value = false;
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove(id: string, alias: string) {
  if (!confirm(`Delete model "${alias}"?`)) return;
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
  await Promise.all([store.refresh(), providers.refresh()]);
  // Default the provider to the first one once the list is loaded.
  if (!form.value.provider_id) form.value.provider_id = providers.options[0]?.value ?? "";
});
</script>

<template>
  <section class="space-y-6">
    <div class="flex items-center justify-between">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Models</h1>
        <p class="mt-1 text-sm text-muted">
          Catalog of AI models the agent can run, with per-token pricing.
        </p>
      </div>
      <button v-if="!showForm" class="btn btn-primary" @click="showForm = true">+ Add model</button>
    </div>

    <form v-if="showForm" class="card space-y-3 p-5" @submit.prevent="submit">
      <div class="grid grid-cols-2 gap-3">
        <label class="flex flex-col">
          <span class="label">Provider</span>
          <select v-model="form.provider_id" required class="select">
            <option v-for="p in providers.options" :key="p.value" :value="p.value">
              {{ p.label }}
            </option>
          </select>
        </label>
        <label class="flex flex-col">
          <span class="label">Alias (display name)</span>
          <input v-model="form.alias" required placeholder="Opus 4.8" class="input" />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">Model id (passed to the CLI)</span>
          <input v-model="form.model_id" required placeholder="claude-opus-4-8" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Input price <span class="text-faint">(USD / 1M)</span></span>
          <input v-model.number="form.input_price" type="number" step="any" min="0" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Output price <span class="text-faint">(USD / 1M)</span></span>
          <input v-model.number="form.output_price" type="number" step="any" min="0" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Cache-write price <span class="text-faint">(USD / 1M)</span></span>
          <input v-model.number="form.cache_write_price" type="number" step="any" min="0" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Cache-read price <span class="text-faint">(USD / 1M)</span></span>
          <input v-model.number="form.cache_read_price" type="number" step="any" min="0" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Effort</span>
          <select v-model="effort" class="select">
            <option value="">— none —</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
          </select>
        </label>
        <div class="flex flex-col justify-end gap-2">
          <label class="flex items-center gap-2">
            <input v-model="form.thinking" type="checkbox" class="h-4 w-4" />
            <span class="text-sm text-ink">Extended thinking</span>
          </label>
          <label class="flex items-center gap-2">
            <input v-model="form.is_default" type="checkbox" class="h-4 w-4" />
            <span class="text-sm text-ink">Global default model</span>
          </label>
        </div>
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
      No models configured. Add one to choose it on services and tasks.
    </div>
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Alias</th>
            <th>Model id</th>
            <th>Provider</th>
            <th class="text-right">In / 1M</th>
            <th class="text-right">Out / 1M</th>
            <th></th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="m in store.list" :key="m.id" class="cursor-pointer" @click="open(m.id)">
            <td>
              <RouterLink
                :to="`/models/${m.id}`"
                class="font-medium text-ink hover:text-accent"
                @click.stop
              >
                {{ m.alias }}
              </RouterLink>
            </td>
            <td class="font-mono text-xs text-muted">{{ m.model_id }}</td>
            <td class="text-xs text-faint">{{ providerName(m.provider_id) }}</td>
            <td class="text-right font-mono text-xs text-muted">${{ m.input_price }}</td>
            <td class="text-right font-mono text-xs text-muted">${{ m.output_price }}</td>
            <td>
              <span v-if="m.is_default" class="pill bg-signal-ok/15 text-signal-ok">default</span>
            </td>
            <td class="text-right">
              <button
                class="text-xs text-signal-danger hover:underline"
                @click.stop="remove(m.id, m.alias)"
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
