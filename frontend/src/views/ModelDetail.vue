<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useModelsStore } from "../stores/models";
import { useProvidersStore } from "../stores/providers";
import type { ModelEffort, UpdateModel } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useModelsStore();
const providers = useProvidersStore();
const router = useRouter();

const draft = ref<UpdateModel>({});
// Effort kept as a plain string so the empty option maps cleanly to null.
const effort = ref<"" | ModelEffort>("");
const saving = ref(false);
const error = ref<string | null>(null);

const detail = computed(() => store.detail);

async function reload() {
  await Promise.all([store.refresh(), providers.refresh()]);
  await store.load(props.id);
  if (store.detail) {
    draft.value = {
      provider_id: store.detail.provider_id,
      model_id: store.detail.model_id,
      alias: store.detail.alias,
      input_price: store.detail.input_price,
      output_price: store.detail.output_price,
      cache_write_price: store.detail.cache_write_price,
      cache_read_price: store.detail.cache_read_price,
      thinking: store.detail.thinking,
      is_default: store.detail.is_default,
      unbound: store.detail.unbound,
    };
    effort.value = store.detail.effort ?? "";
  }
}

onMounted(reload);
watch(() => props.id, reload);

async function save() {
  saving.value = true;
  error.value = null;
  try {
    const body: UpdateModel = { ...draft.value, effort: effort.value || null };
    await store.update(props.id, body);
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove() {
  if (!detail.value) return;
  if (!confirm(`Delete model "${detail.value.alias}"?`)) return;
  try {
    await store.remove(props.id);
    router.push({ name: "models" });
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
    <RouterLink to="/models" class="inline-block text-sm text-muted hover:text-accent">← Models</RouterLink>

    <header class="flex items-center gap-3">
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ detail.alias }}</h1>
      <span class="font-mono text-sm text-faint">{{ detail.model_id }}</span>
      <span v-if="detail.is_default" class="pill bg-signal-ok/15 text-signal-ok">default</span>
      <span
        v-if="detail.unbound"
        class="pill border-signal-danger bg-signal-danger/15 font-bold text-signal-danger"
        title="Runs every command with no approval"
      >
        ⚠ UNBOUND
      </span>
      <button class="btn btn-danger btn-sm ml-auto" @click="remove">Delete model</button>
    </header>

    <form class="card space-y-3 p-5" @submit.prevent="save">
      <h2 class="text-sm font-semibold">Settings</h2>
      <div class="grid grid-cols-2 gap-3">
        <div>
          <label class="label">Provider</label>
          <select v-model="draft.provider_id" class="select">
            <option v-for="p in providers.options" :key="p.value" :value="p.value">
              {{ p.label }}
            </option>
          </select>
        </div>
        <div>
          <label class="label">Alias (unique display name)</label>
          <input v-model="draft.alias" class="input" />
          <p class="mt-1 text-xs text-faint">Must be unique across all models.</p>
        </div>
        <div class="col-span-2">
          <label class="label">Model id (passed to the CLI)</label>
          <input v-model="draft.model_id" class="input font-mono" />
          <p class="mt-1 text-xs text-faint">May repeat across aliases.</p>
        </div>
        <div>
          <label class="label">Input price <span class="text-faint">(USD / 1M)</span></label>
          <input v-model.number="draft.input_price" type="number" step="any" min="0" class="input font-mono" />
        </div>
        <div>
          <label class="label">Output price <span class="text-faint">(USD / 1M)</span></label>
          <input v-model.number="draft.output_price" type="number" step="any" min="0" class="input font-mono" />
        </div>
        <div>
          <label class="label">Cache-write price <span class="text-faint">(USD / 1M)</span></label>
          <input v-model.number="draft.cache_write_price" type="number" step="any" min="0" class="input font-mono" />
        </div>
        <div>
          <label class="label">Cache-read price <span class="text-faint">(USD / 1M)</span></label>
          <input v-model.number="draft.cache_read_price" type="number" step="any" min="0" class="input font-mono" />
        </div>
        <div>
          <label class="label">Effort</label>
          <select v-model="effort" class="select">
            <option value="">— none —</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
          </select>
        </div>
        <div>
          <label class="label">Extended thinking</label>
          <select v-model="draft.thinking" class="select">
            <option :value="null">Default</option>
            <option :value="true">On</option>
            <option :value="false">Off</option>
          </select>
        </div>
        <div class="flex flex-col justify-end gap-2">
          <label class="flex items-center gap-2">
            <input v-model="draft.is_default" type="checkbox" class="h-4 w-4" />
            <span class="text-sm text-ink">Global default model</span>
          </label>
        </div>
      </div>

      <div class="rounded-md border border-signal-danger/60 bg-signal-danger/5 p-3">
        <label class="flex items-start gap-2">
          <input v-model="draft.unbound" type="checkbox" class="mt-0.5 h-4 w-4 accent-signal-danger" />
          <span>
            <span class="block text-sm font-bold text-signal-danger">⚠ Unbound (dangerous)</span>
            <span class="mt-0.5 block text-xs text-muted">
              Runs every command — including arbitrary shell — with no approval.
              Only enable for fully trusted, sandboxed setups.
            </span>
          </span>
        </label>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <div class="flex gap-2">
        <button type="submit" :disabled="saving" class="btn btn-primary">
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <RouterLink :to="{ name: 'models' }" class="btn btn-ghost">Back</RouterLink>
      </div>
    </form>

    <p class="font-mono text-xs text-faint">
      Updated {{ new Date(detail.updated_at).toLocaleString() }} ·
      created {{ new Date(detail.created_at).toLocaleString() }}
    </p>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
