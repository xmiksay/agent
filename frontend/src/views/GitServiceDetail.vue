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
    <header class="flex items-center gap-3">
      <ProviderBadge :provider="detail.kind" />
      <h1 class="text-xl font-semibold">{{ detail.display_name }}</h1>
      <span class="text-sm text-gray-500 font-mono">{{ detail.slug }}</span>
      <button
        class="ml-auto text-xs text-red-600 hover:underline"
        @click="remove"
      >
        delete service
      </button>
    </header>

    <section class="bg-white p-4 rounded shadow-sm space-y-2">
      <h2 class="font-medium text-sm">Webhook URL</h2>
      <p class="text-xs text-gray-500">
        Paste this into the {{ detail.kind === "github" ? "GitHub" : "GitLab" }}
        webhook settings, alongside the secret you saved.
      </p>
      <div class="flex gap-2">
        <input
          readonly
          :value="fullWebhookUrl"
          class="flex-1 border rounded p-2 font-mono text-xs bg-gray-50"
        />
        <button
          type="button"
          class="rounded border px-3 py-1.5 text-sm hover:bg-gray-50"
          @click="copyWebhook"
        >
          {{ copied ? "Copied" : "Copy" }}
        </button>
      </div>
    </section>

    <form class="bg-white p-4 rounded shadow-sm space-y-3" @submit.prevent="save">
      <h2 class="font-medium text-sm">Settings</h2>
      <div class="grid grid-cols-2 gap-3 text-sm">
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">Display name</span>
          <input v-model="draft.display_name" class="border rounded p-2" />
        </label>
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">Base URL</span>
          <input v-model="draft.base_url" type="url" class="border rounded p-2 font-mono" />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">Bot username</span>
          <input v-model="draft.bot_username" class="border rounded p-2 font-mono" />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">
            Personal access token
            <span class="text-gray-400">(leave blank to keep)</span>
          </span>
          <input
            v-model="tokenDraft"
            type="password"
            autocomplete="new-password"
            class="border rounded p-2 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">
            Webhook secret
            <span class="text-gray-400">(leave blank to keep)</span>
          </span>
          <input
            v-model="webhookSecretDraft"
            type="password"
            autocomplete="new-password"
            class="border rounded p-2 font-mono"
          />
        </label>
      </div>

      <p v-if="error" class="text-sm text-red-600">{{ error }}</p>

      <div class="flex gap-2">
        <button
          type="submit"
          :disabled="saving"
          class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <RouterLink
          :to="{ name: 'git-services' }"
          class="rounded border px-4 py-2 hover:bg-gray-50"
        >
          Back
        </RouterLink>
      </div>
    </form>

    <p class="text-xs text-gray-400">
      Updated {{ new Date(detail.updated_at).toLocaleString() }} ·
      created {{ new Date(detail.created_at).toLocaleString() }}
    </p>
  </section>
  <p v-else class="text-gray-500">Loading…</p>
</template>
