<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useGitServicesStore } from "../stores/git_services";
import ProviderBadge from "../components/ProviderBadge.vue";
import type { UpdateGitService } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useGitServicesStore();
const router = useRouter();

const draft = ref<UpdateGitService>({});
const tokenDraft = ref("");
const webhookSecretDraft = ref("");
const saving = ref(false);
const error = ref<string | null>(null);
const copied = ref(false);

const detail = computed(() => store.detail);

async function reload() {
  await store.load(props.id);
  if (store.detail) {
    draft.value = {
      display_name: store.detail.display_name,
      base_url: store.detail.base_url,
      bot_username: store.detail.bot_username,
      autofire: store.detail.autofire,
    };
    tokenDraft.value = "";
    webhookSecretDraft.value = "";
  }
}

onMounted(reload);
watch(() => props.id, reload);

const fullWebhookUrl = computed(() => {
  if (!detail.value) return "";
  return `${window.location.origin}${detail.value.webhook_path}`;
});

async function copyWebhook() {
  if (!fullWebhookUrl.value) return;
  try {
    await navigator.clipboard.writeText(fullWebhookUrl.value);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    // Clipboard API can be denied; the user can still select and copy.
  }
}

async function save() {
  saving.value = true;
  error.value = null;
  try {
    const body: UpdateGitService = { ...draft.value };
    if (tokenDraft.value) body.token = tokenDraft.value;
    if (webhookSecretDraft.value) body.webhook_secret = webhookSecretDraft.value;
    await store.update(props.id, body);
    tokenDraft.value = "";
    webhookSecretDraft.value = "";
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove() {
  if (!detail.value) return;
  if (!confirm(`Delete git service "${detail.value.slug}"?`)) return;
  try {
    await store.remove(props.id);
    router.push({ name: "git-services" });
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
    <RouterLink to="/git_services" class="inline-block text-sm text-muted hover:text-accent">← Services</RouterLink>

    <header class="flex items-center gap-3">
      <ProviderBadge :provider="detail.kind" />
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ detail.display_name }}</h1>
      <span class="font-mono text-sm text-faint">{{ detail.slug }}</span>
      <button class="btn btn-danger btn-sm ml-auto" @click="remove">Delete service</button>
    </header>

    <section class="card space-y-2 p-5">
      <h2 class="text-sm font-semibold">Webhook URL</h2>
      <p class="text-xs text-muted">
        Paste this into the {{ detail.kind === "github" ? "GitHub" : "GitLab" }}
        webhook settings, alongside the secret you saved.
      </p>
      <div class="flex gap-2">
        <input readonly :value="fullWebhookUrl" class="input flex-1 font-mono text-xs" />
        <button type="button" class="btn btn-ghost" @click="copyWebhook">
          {{ copied ? "Copied" : "Copy" }}
        </button>
      </div>
    </section>

    <form class="card space-y-3 p-5" @submit.prevent="save">
      <h2 class="text-sm font-semibold">Settings</h2>
      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2">
          <label class="label">Display name</label>
          <input v-model="draft.display_name" class="input" />
        </div>
        <div class="col-span-2">
          <label class="label">Base URL</label>
          <input v-model="draft.base_url" type="url" class="input font-mono" />
        </div>
        <div>
          <label class="label">Bot username</label>
          <input v-model="draft.bot_username" class="input font-mono" />
        </div>
        <div>
          <label class="label">Personal access token <span class="text-faint">(leave blank to keep)</span></label>
          <input
            v-model="tokenDraft"
            type="password"
            autocomplete="new-password"
            class="input font-mono"
          />
        </div>
        <div class="col-span-2">
          <label class="label">Webhook secret <span class="text-faint">(leave blank to keep)</span></label>
          <input
            v-model="webhookSecretDraft"
            type="password"
            autocomplete="new-password"
            class="input font-mono"
          />
        </div>
        <div class="col-span-2">
          <label class="flex items-center gap-2">
            <input v-model="draft.autofire" type="checkbox" class="h-4 w-4" />
            <span class="text-sm text-ink">Autofire</span>
          </label>
          <p class="mt-1 text-xs text-muted">
            Auto-confirms and runs new tasks from this service immediately, instead of leaving them pending.
          </p>
        </div>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <div class="flex gap-2">
        <button type="submit" :disabled="saving" class="btn btn-primary">
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <RouterLink :to="{ name: 'git-services' }" class="btn btn-ghost">Back</RouterLink>
      </div>
    </form>

    <p class="font-mono text-xs text-faint">
      Updated {{ new Date(detail.updated_at).toLocaleString() }} ·
      created {{ new Date(detail.created_at).toLocaleString() }}
    </p>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
