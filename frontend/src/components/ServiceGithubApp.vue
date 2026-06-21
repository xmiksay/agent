<script setup lang="ts">
// GitHub App section: install/manage the App and let the bot self-sync the
// installation + app-level webhook. Shown only once an App-kind service is saved.
import { ref } from "vue";
import { extractErrorMessage } from "../util/error";
import { servicesApi } from "../api/services";

const props = defineProps<{ id: string; appInstalled: boolean }>();
const emit = defineEmits<{ reload: []; error: [string] }>();

const installing = ref(false);
const syncing = ref(false);
const syncResult = ref<{ ok: boolean; text: string } | null>(null);

async function installApp() {
  installing.value = true;
  emit("error", "");
  try {
    const { install_url } = await servicesApi.githubAppInstallUrl(props.id);
    window.location.href = install_url;
  } catch (e: unknown) {
    emit("error", extractErrorMessage(e));
    installing.value = false;
  }
}

async function syncApp() {
  syncing.value = true;
  syncResult.value = null;
  try {
    const res = await servicesApi.githubAppSync(props.id);
    syncResult.value = { ok: true, text: res.message };
    emit("reload");
  } catch (e: unknown) {
    syncResult.value = { ok: false, text: extractErrorMessage(e) };
  } finally {
    syncing.value = false;
  }
}
</script>

<template>
  <section class="card space-y-3 p-5">
    <div class="flex items-center gap-2">
      <h2 class="text-sm font-semibold">GitHub App</h2>
      <span
        class="rounded px-2 py-0.5 text-xs font-medium"
        :class="appInstalled ? 'bg-signal-ok/15 text-signal-ok' : 'bg-signal-auth/15 text-signal-auth'"
      >
        {{ appInstalled ? "Installed" : "Not installed" }}
      </span>
    </div>
    <p class="text-xs text-muted">
      Install the App on the repos it should act on — that records the installation id used to
      mint short-lived tokens. Configure the webhook once at the app level (URL above + secret);
      per-repo hooks are skipped for App services.
    </p>
    <div class="flex flex-wrap gap-2">
      <button type="button" class="btn btn-primary" :disabled="installing" @click="installApp">
        {{ installing ? "Redirecting…" : appInstalled ? "Reinstall / manage" : "Install GitHub App" }}
      </button>
      <button type="button" class="btn btn-ghost" :disabled="syncing" @click="syncApp">
        {{ syncing ? "Syncing…" : "Sync App" }}
      </button>
    </div>
    <p class="text-xs text-muted">
      <strong>Sync App</strong> lets the bot finish setup itself with the App key: it discovers the
      installation (no redirect needed) and registers the app-level webhook (URL + secret) for you.
      Install the App on your repos first.
    </p>
    <p
      v-if="syncResult"
      class="text-xs"
      :class="syncResult.ok ? 'text-signal-ok' : 'text-signal-danger'"
    >
      {{ syncResult.text }}
    </p>
  </section>
</template>
