<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useGitServicesStore } from "../stores/git_services";
import ProviderBadge from "../components/ProviderBadge.vue";
import type { NewGitService, ProviderKind } from "../types/api";

const store = useGitServicesStore();
const router = useRouter();

function open(id: string) {
  router.push(`/git_services/${id}`);
}

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
    <div class="flex items-center justify-between">
      <div>
        <h1 class="font-display text-2xl font-bold tracking-tight">Git services</h1>
        <p class="mt-1 text-sm text-muted">
          Provider connections — tokens and webhook secrets stay write-only.
        </p>
      </div>
      <button v-if="!showForm" class="btn btn-primary" @click="showForm = true">+ Add service</button>
    </div>

    <form v-if="showForm" class="card space-y-3 p-5" @submit.prevent="submit">
      <div class="grid grid-cols-2 gap-3">
        <label class="flex flex-col">
          <span class="label">Kind</span>
          <select v-model="form.kind" class="select" @change="onKindChange">
            <option value="gitlab">GitLab</option>
            <option value="github" :disabled="hasGithub">
              GitHub{{ hasGithub ? " (already configured)" : "" }}
            </option>
          </select>
        </label>
        <label class="flex flex-col">
          <span class="label">Slug (URL-safe, unique)</span>
          <input
            v-model="form.slug"
            required
            pattern="[A-Za-z0-9_-]+"
            placeholder="e.g. main, work, personal"
            class="input font-mono"
          />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">Display name</span>
          <input v-model="form.display_name" required placeholder="Work GitLab" class="input" />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">
            Base URL ({{ form.kind === "github" ? "REST API" : "GitLab instance" }})
          </span>
          <input v-model="form.base_url" required type="url" class="input font-mono" />
        </label>
        <label class="flex flex-col">
          <span class="label">Bot username</span>
          <input
            v-model="form.bot_username"
            required
            placeholder="@-mention to match in comments"
            class="input font-mono"
          />
        </label>
        <label class="flex flex-col">
          <span class="label">Personal access token</span>
          <input
            v-model="form.token"
            required
            type="password"
            autocomplete="new-password"
            class="input font-mono"
          />
        </label>
        <label class="col-span-2 flex flex-col">
          <span class="label">Webhook secret</span>
          <input
            v-model="form.webhook_secret"
            required
            type="password"
            autocomplete="new-password"
            class="input font-mono"
          />
        </label>
      </div>

      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>

      <p class="text-xs text-muted">{{ webhookHelp(form.kind) }}</p>

      <div class="flex gap-2">
        <button type="submit" :disabled="saving" class="btn btn-primary">
          {{ saving ? "Saving…" : "Create" }}
        </button>
        <button type="button" class="btn btn-ghost" @click="showForm = false">Cancel</button>
      </div>
    </form>

    <div v-if="store.loading" class="text-muted">Loading…</div>
    <div v-else-if="!store.list.length" class="card p-10 text-center text-faint">
      No git services configured. Add one to start receiving webhooks.
    </div>
    <div v-else class="card overflow-x-auto">
      <table class="tbl">
        <thead>
          <tr>
            <th>Kind</th>
            <th>Name</th>
            <th>Slug</th>
            <th>Base URL</th>
            <th class="text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="s in store.list" :key="s.id" class="cursor-pointer" @click="open(s.id)">
            <td><ProviderBadge :provider="s.kind" /></td>
            <td>
              <RouterLink
                :to="`/git_services/${s.id}`"
                class="font-medium text-ink hover:text-accent"
                @click.stop
              >
                {{ s.display_name }}
              </RouterLink>
            </td>
            <td class="font-mono text-xs text-muted">{{ s.slug }}</td>
            <td class="max-w-[260px] truncate font-mono text-xs text-faint">{{ s.base_url }}</td>
            <td class="text-right">
              <button
                class="text-xs text-signal-danger hover:underline"
                @click.stop="remove(s.id, s.slug)"
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
