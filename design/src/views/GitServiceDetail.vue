<script setup lang="ts">
import { reactive, ref } from "vue";
import { RouterLink } from "vue-router";
import ProviderBadge from "../components/ProviderBadge.vue";
import { services } from "../fixtures";

const props = defineProps<{ id: string }>();
const detail = services.find((s) => s.id === props.id);
const draft = reactive({
  display_name: detail?.display_name ?? "",
  base_url: detail?.base_url ?? "",
  bot_username: detail?.bot_username ?? "",
});
const fullWebhookUrl = `https://agent.local${detail?.webhook_path ?? ""}`;
const copied = ref(false);
function copyWebhook() {
  navigator.clipboard?.writeText(fullWebhookUrl);
  copied.value = true;
  setTimeout(() => (copied.value = false), 1500);
}
</script>

<template>
  <section v-if="detail" class="space-y-6">
    <RouterLink to="/git_services" class="inline-block text-sm text-muted hover:text-accent">← Services</RouterLink>

    <header class="flex items-center gap-3">
      <ProviderBadge :provider="detail.kind" />
      <h1 class="font-display text-2xl font-bold tracking-tight">{{ detail.display_name }}</h1>
      <span class="font-mono text-sm text-faint">{{ detail.slug }}</span>
      <button class="btn btn-danger btn-sm ml-auto">Delete service</button>
    </header>

    <section class="card space-y-2 p-5">
      <h2 class="text-sm font-semibold">Webhook URL</h2>
      <p class="text-xs text-muted">
        Paste this into the {{ detail.kind === "github" ? "GitHub" : "GitLab" }}
        webhook settings, alongside the secret you saved.
      </p>
      <div class="flex gap-2">
        <input readonly :value="fullWebhookUrl" class="input flex-1 font-mono text-xs" />
        <button class="btn btn-ghost" @click="copyWebhook">{{ copied ? "Copied" : "Copy" }}</button>
      </div>
    </section>

    <form class="card space-y-3 p-5" @submit.prevent>
      <h2 class="text-sm font-semibold">Settings</h2>
      <div class="grid grid-cols-2 gap-3">
        <div class="col-span-2"><label class="label">Display name</label><input v-model="draft.display_name" class="input" /></div>
        <div class="col-span-2"><label class="label">Base URL</label><input v-model="draft.base_url" type="url" class="input font-mono" /></div>
        <div><label class="label">Bot username</label><input v-model="draft.bot_username" class="input font-mono" /></div>
        <div><label class="label">Token <span class="text-faint">(blank = keep)</span></label><input type="password" autocomplete="new-password" class="input font-mono" /></div>
        <div class="col-span-2"><label class="label">Webhook secret <span class="text-faint">(blank = keep)</span></label><input type="password" autocomplete="new-password" class="input font-mono" /></div>
      </div>
      <div class="flex gap-2">
        <button type="submit" class="btn btn-primary">Save</button>
        <RouterLink to="/git_services" class="btn btn-ghost">Back</RouterLink>
      </div>
    </form>
  </section>
  <p v-else class="text-faint">Service not found.</p>
</template>
