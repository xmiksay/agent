<script setup lang="ts">
import { extractErrorMessage } from "../util/error";
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useServicesStore } from "../stores/services";
import { useModelsStore } from "../stores/models";
import ProviderBadge from "../components/ProviderBadge.vue";
import TriggerGatingGrid from "../components/TriggerGatingGrid.vue";
import ServiceGithubApp from "../components/ServiceGithubApp.vue";
import ServiceGitlabToken from "../components/ServiceGitlabToken.vue";
import { TRIGGER_TYPES, seedTriggers } from "../util/triggerTypes";
import type { AuthKind, TriggerConfig, UpdateService } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useServicesStore();
const models = useModelsStore();
const router = useRouter();

const draft = ref<UpdateService>({});
// trigger_type -> model id; "" means unmapped and is dropped on submit.
const triggerModels = ref<Record<string, string>>({});
// Full 5-entry gating map; submitted wholesale (replaces all rows).
const triggerGating = ref<Record<string, TriggerConfig>>(seedTriggers());
const tokenDraft = ref("");
const webhookSecretDraft = ref("");
const appIdDraft = ref("");
const privateKeyDraft = ref("");
const authKindDraft = ref<AuthKind>("pat");
const saving = ref(false);
const error = ref<string | null>(null);
const copied = ref(false);
const generatedSecret = ref<string | null>(null);

const detail = computed(() => store.detail);
// GitHub-only: `app` is rejected for GitLab, so the selector is hidden there.
const isGithub = computed(() => detail.value?.kind === "github");
const isGitlab = computed(() => detail.value?.kind === "gitlab");
const gitlabToken = computed(() => detail.value?.gitlab_token ?? null);
// The currently *selected* auth kind drives which credential inputs show, so a
// PAT service can be converted to App in place. The install section below keys
// off the *saved* kind instead — you install only after the switch is saved.
const isAppDraft = computed(() => isGithub.value && authKindDraft.value === "app");
const isAppSaved = computed(() => detail.value?.auth_kind === "app");

async function reload() {
  await store.load(props.id);
  if (store.detail) {
    draft.value = {
      display_name: store.detail.display_name,
      base_url: store.detail.base_url,
      bot_username: store.detail.bot_username,
      autofire: store.detail.autofire,
      trigger_mode: store.detail.trigger_mode,
      trigger_label: store.detail.trigger_label,
    };
    triggerModels.value = { ...store.detail.models };
    triggerGating.value = seedTriggers(store.detail.triggers);
    authKindDraft.value = store.detail.auth_kind;
    tokenDraft.value = "";
    webhookSecretDraft.value = "";
    appIdDraft.value = "";
    privateKeyDraft.value = "";
  }
}

onMounted(() => {
  reload();
  models.refresh();
});
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
    const body: UpdateService = {
      ...draft.value,
      models: collectModels(),
      triggers: triggerGating.value,
    };
    if (tokenDraft.value) body.token = tokenDraft.value;
    if (webhookSecretDraft.value) body.webhook_secret = webhookSecretDraft.value;
    if (isGithub.value) body.auth_kind = authKindDraft.value;
    if (isAppDraft.value) {
      // Switching PAT → App needs fresh credentials; an existing App can leave
      // them blank to keep. Replacing the bundle drops the recorded installation
      // (operator reinstalls afterward).
      const switchingToApp = !isAppSaved.value;
      if (switchingToApp || appIdDraft.value || privateKeyDraft.value) {
        if (!appIdDraft.value || !privateKeyDraft.value) {
          throw new Error("provide both App ID and private key");
        }
        body.app_credentials = { app_id: appIdDraft.value, private_key: privateKeyDraft.value };
      }
    }
    const updated = await store.update(props.id, body);
    generatedSecret.value = updated.generated_webhook_secret ?? null;
    tokenDraft.value = "";
    webhookSecretDraft.value = "";
    appIdDraft.value = "";
    privateKeyDraft.value = "";
  } catch (e: unknown) {
    error.value = extractErrorMessage(e);
  } finally {
    saving.value = false;
  }
}

// Drop unset entries so the map only carries real trigger_type -> model id pairs.
function collectModels(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(triggerModels.value)) {
    if (v) out[k] = v;
  }
  return out;
}

// Children clear the banner with an empty string and surface failures with a message.
function setError(msg: string) {
  error.value = msg || null;
}

async function remove() {
  if (!detail.value) return;
  if (!confirm(`Delete service "${detail.value.slug}"?`)) return;
  try {
    await store.remove(props.id);
    router.push({ name: "services" });
  } catch (e: unknown) {
    alert(extractErrorMessage(e));
  }
}</script>

<template>
  <section v-if="detail" class="space-y-6">
    <RouterLink to="/services" class="inline-block text-sm text-muted hover:text-accent">← Services</RouterLink>

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

    <ServiceGithubApp
      v-if="isAppSaved"
      :id="props.id"
      :app-installed="detail.app_installed"
      @reload="reload"
      @error="setError"
    />

    <ServiceGitlabToken
      v-if="isGitlab"
      :id="props.id"
      :slug="detail.slug"
      :gitlab-token="gitlabToken"
      @reload="reload"
      @error="setError"
    />

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
        <div v-if="isGithub">
          <label class="label">Authentication</label>
          <select v-model="authKindDraft" class="select">
            <option value="pat">Personal access token</option>
            <option value="app">GitHub App</option>
          </select>
        </div>
        <div v-if="!isAppDraft">
          <label class="label">Personal access token <span class="text-faint">(leave blank to keep)</span></label>
          <input
            v-model="tokenDraft"
            type="password"
            autocomplete="new-password"
            class="input font-mono"
          />
        </div>
        <div v-if="isAppDraft">
          <label class="label">
            App ID
            <span class="text-faint">{{ isAppSaved ? "(leave blank to keep)" : "" }}</span>
          </label>
          <input v-model="appIdDraft" class="input font-mono" placeholder="123456" />
        </div>
        <div v-if="isAppDraft" class="col-span-2">
          <label class="label">
            Private key (PEM) <span class="text-faint">(leave blank to keep — replacing it resets the install)</span>
          </label>
          <textarea
            v-model="privateKeyDraft"
            rows="4"
            autocomplete="new-password"
            class="input font-mono text-xs"
            placeholder="-----BEGIN RSA PRIVATE KEY-----"
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
        <div>
          <label class="label">Trigger</label>
          <select v-model="draft.trigger_mode" class="select">
            <option value="assignee">Assignee</option>
            <option value="label">Label</option>
            <option value="both">Both</option>
          </select>
        </div>
        <div v-if="draft.trigger_mode !== 'assignee'">
          <label class="label">Trigger label</label>
          <input v-model="draft.trigger_label" class="input font-mono" placeholder="agent" />
          <p class="mt-1 text-xs text-muted">
            Issues with this label trigger the agent — works for GitHub App identities, which can't
            be assignees.
          </p>
        </div>
        <div class="col-span-2">
          <label class="label">Models per task type</label>
          <p class="mb-2 text-xs text-muted">
            Pick a model per trigger type. Unset types fall back to the global default.
          </p>
          <div class="grid grid-cols-2 gap-3">
            <div v-for="t in TRIGGER_TYPES" :key="t.value">
              <label class="label">{{ t.label }}</label>
              <select v-model="triggerModels[t.value]" class="select">
                <option value="">— none —</option>
                <option v-for="m in models.options" :key="m.value" :value="m.value">
                  {{ m.label }}
                </option>
              </select>
            </div>
          </div>
        </div>
        <div class="col-span-2">
          <TriggerGatingGrid v-model="triggerGating" />
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
        <RouterLink :to="{ name: 'services' }" class="btn btn-ghost">Back</RouterLink>
      </div>
    </form>

    <div v-if="generatedSecret" class="card space-y-2 border border-signal-auth/40 p-5">
      <h2 class="text-sm font-semibold text-signal-auth">Webhook secret generated</h2>
      <p class="text-xs text-muted">
        Save this now — it's shown once and never again. For a GitHub App, paste it into the App's
        webhook settings.
      </p>
      <div class="flex gap-2">
        <input readonly :value="generatedSecret" class="input flex-1 font-mono text-xs" />
        <button type="button" class="btn btn-ghost" @click="generatedSecret = null">Dismiss</button>
      </div>
    </div>

    <p class="font-mono text-xs text-faint">
      Updated {{ new Date(detail.updated_at).toLocaleString() }} ·
      created {{ new Date(detail.created_at).toLocaleString() }}
    </p>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
