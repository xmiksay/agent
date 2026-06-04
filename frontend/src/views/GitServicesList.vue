<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useGitServicesStore } from "../stores/git_services";
import ProviderBadge from "../components/ProviderBadge.vue";
import type { NewGitService, ProviderKind } from "../types/api";

const store = useGitServicesStore();

const showForm = ref(false);
const form = ref<NewGitService>(blank());
const saving = ref(false);
const error = ref<string | null>(null);

function blank(): NewGitService {
  return {
    kind: "gitlab",
    slug: "",
    display_name: "",
    base_url: "https://gitlab.com",
    token: "",
    webhook_secret: "",
    bot_username: "",
  };
}

const hasGithub = computed(() => store.list.some((s) => s.kind === "github"));

function onKindChange() {
  // Helpful defaults so users don't have to guess the API base.
  if (form.value.kind === "github") {
    form.value.base_url = "https://api.github.com";
  } else if (!form.value.base_url || form.value.base_url === "https://api.github.com") {
    form.value.base_url = "https://gitlab.com";
  }
}

async function submit() {
  saving.value = true;
  error.value = null;
  try {
    await store.create(form.value);
    form.value = blank();
    showForm.value = false;
  } catch (e: unknown) {
    error.value = extractMessage(e);
  } finally {
    saving.value = false;
  }
}

async function remove(id: string, slug: string) {
  if (!confirm(`Delete git service "${slug}"? Projects keep their data but lose their link.`)) {
    return;
  }
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

function webhookHelp(kind: ProviderKind) {
  return kind === "github"
    ? "Configure as a Webhook in GitHub: Content type application/json, your webhook_secret as the secret, events: issues, issue_comment, pull_request, pull_request_review."
    : "Configure as a Webhook in GitLab: secret token = the webhook_secret, triggers: Issues / Merge requests / Comments.";
}

onMounted(() => store.refresh());
</script>

<template>
  <section class="space-y-6">
    <div class="flex items-center gap-4">
      <h1 class="text-2xl font-semibold">Git services</h1>
      <button
        v-if="!showForm"
        class="ml-auto rounded bg-blue-600 text-white px-3 py-1.5 text-sm hover:bg-blue-700"
        @click="showForm = true"
      >
        Add service
      </button>
    </div>

    <form
      v-if="showForm"
      class="bg-white p-4 rounded shadow-sm space-y-3"
      @submit.prevent="submit"
    >
      <div class="grid grid-cols-2 gap-3 text-sm">
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">Kind</span>
          <select v-model="form.kind" class="border rounded p-2" @change="onKindChange">
            <option value="gitlab">GitLab</option>
            <option value="github" :disabled="hasGithub">
              GitHub{{ hasGithub ? " (already configured)" : "" }}
            </option>
          </select>
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">Slug (URL-safe, unique)</span>
          <input
            v-model="form.slug"
            required
            pattern="[A-Za-z0-9_-]+"
            placeholder="e.g. main, work, personal"
            class="border rounded p-2 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">Display name</span>
          <input
            v-model="form.display_name"
            required
            placeholder="Work GitLab"
            class="border rounded p-2"
          />
        </label>
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">
            Base URL ({{ form.kind === "github" ? "REST API" : "GitLab instance" }})
          </span>
          <input
            v-model="form.base_url"
            required
            type="url"
            class="border rounded p-2 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">Bot username</span>
          <input
            v-model="form.bot_username"
            required
            placeholder="@-mention to match in comments"
            class="border rounded p-2 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-xs text-gray-500">Personal access token</span>
          <input
            v-model="form.token"
            required
            type="password"
            autocomplete="new-password"
            class="border rounded p-2 font-mono"
          />
        </label>
        <label class="flex flex-col gap-1 col-span-2">
          <span class="text-xs text-gray-500">Webhook secret</span>
          <input
            v-model="form.webhook_secret"
            required
            type="password"
            autocomplete="new-password"
            class="border rounded p-2 font-mono"
          />
        </label>
      </div>

      <p v-if="error" class="text-sm text-red-600">{{ error }}</p>

      <p class="text-xs text-gray-500">{{ webhookHelp(form.kind) }}</p>

      <div class="flex gap-2">
        <button
          type="submit"
          :disabled="saving"
          class="rounded bg-blue-600 text-white px-4 py-2 hover:bg-blue-700 disabled:opacity-60"
        >
          {{ saving ? "Saving…" : "Create" }}
        </button>
        <button
          type="button"
          class="rounded border px-4 py-2 hover:bg-gray-50"
          @click="showForm = false"
        >
          Cancel
        </button>
      </div>
    </form>

    <div v-if="store.loading" class="text-gray-500">Loading…</div>
    <ul v-else class="space-y-2">
      <li
        v-for="s in store.list"
        :key="s.id"
        class="bg-white rounded shadow-sm px-4 py-3 flex items-center gap-3"
      >
        <ProviderBadge :provider="s.kind" />
        <RouterLink :to="`/git_services/${s.id}`" class="font-medium">
          {{ s.display_name }}
        </RouterLink>
        <span class="text-xs text-gray-500 font-mono">{{ s.slug }}</span>
        <span class="text-xs text-gray-400 truncate">{{ s.base_url }}</span>
        <button
          class="ml-auto text-xs text-red-600 hover:underline"
          @click="remove(s.id, s.slug)"
        >
          delete
        </button>
      </li>
      <li v-if="!store.list.length && !store.loading" class="text-gray-500">
        No git services configured. Add one to start receiving webhooks.
      </li>
    </ul>
  </section>
</template>
