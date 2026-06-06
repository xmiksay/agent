<script setup lang="ts">
import { ref } from "vue";
import { useSessionStore } from "../stores/session";
import { probeAuth } from "../api/client";

const session = useSessionStore();
const draft = ref(session.token ?? "");
const busy = ref(false);
const error = ref<string | null>(null);

async function save() {
  error.value = null;
  session.set(draft.value);
  if (!session.hasToken) {
    error.value = "Token cannot be empty.";
    return;
  }
  busy.value = true;
  try {
    const ok = await probeAuth();
    if (!ok) error.value = "Server rejected the token.";
  } finally {
    busy.value = false;
  }
}

function forget() {
  session.clear();
  draft.value = "";
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-canvas/80 backdrop-blur-sm">
    <div class="card w-full max-w-md space-y-4 border-line-2 p-6 shadow-[0_30px_80px_-20px_rgba(0,0,0,0.85)]">
      <div class="flex items-center gap-2.5">
        <span class="led led-pulse text-accent" />
        <h2 class="font-display text-lg font-bold">Sign in</h2>
      </div>
      <p class="text-sm text-muted">
        Paste the API bearer token (the value of
        <code class="rounded border border-line bg-panel-2 px-1 py-0.5 font-mono text-xs text-ink">API_BEARER_TOKEN</code>
        on the server). Stored locally in your browser.
      </p>
      <input
        v-model="draft"
        type="password"
        autocomplete="off"
        placeholder="Bearer token"
        class="input font-mono"
        @keydown.enter="save"
      />
      <p v-if="error" class="text-sm text-signal-danger">{{ error }}</p>
      <div class="flex gap-2">
        <button class="btn btn-primary" :disabled="busy" @click="save">
          {{ busy ? "Checking…" : "Save" }}
        </button>
        <button v-if="session.hasToken" class="btn btn-ghost" @click="forget">Forget</button>
      </div>
    </div>
  </div>
</template>
