<script setup lang="ts">
// GitLab bot access token: mint a Group/Project Access Token so the agent acts
// as its own bot, then rotate it on demand. Shown only for GitLab services.
import { ref } from "vue";
import { extractErrorMessage } from "../util/error";
import { servicesApi } from "../api/services";
import type { GitLabTokenMeta, GitLabTokenScope } from "../types/api";

const props = defineProps<{
  id: string;
  slug: string;
  gitlabToken: GitLabTokenMeta | null;
}>();
const emit = defineEmits<{ reload: []; error: [string] }>();

const provScope = ref<GitLabTokenScope>("group");
const provNamespace = ref("");
const provName = ref("");
const provExpiry = ref("");
const provisioning = ref(false);
const rotating = ref(false);

async function provisionToken() {
  if (!provNamespace.value.trim()) {
    emit("error", "enter the group or project path");
    return;
  }
  provisioning.value = true;
  emit("error", "");
  try {
    await servicesApi.provisionGitlabToken(props.id, {
      scope: provScope.value,
      namespace: provNamespace.value.trim(),
      name: provName.value.trim() || undefined,
      expires_at: provExpiry.value || undefined,
    });
    emit("reload");
  } catch (e: unknown) {
    emit("error", extractErrorMessage(e));
  } finally {
    provisioning.value = false;
  }
}

async function rotateToken() {
  if (!confirm("Rotate the bot token? The current token is revoked immediately.")) return;
  rotating.value = true;
  emit("error", "");
  try {
    await servicesApi.rotateGitlabToken(props.id);
    emit("reload");
  } catch (e: unknown) {
    emit("error", extractErrorMessage(e));
  } finally {
    rotating.value = false;
  }
}
</script>

<template>
  <section class="card space-y-3 p-5">
    <div class="flex items-center gap-2">
      <h2 class="text-sm font-semibold">Bot access token</h2>
      <span
        class="rounded px-2 py-0.5 text-xs font-medium"
        :class="gitlabToken ? 'bg-signal-ok/15 text-signal-ok' : 'bg-signal-auth/15 text-signal-auth'"
      >
        {{ gitlabToken ? "Provisioned" : "Not provisioned" }}
      </span>
    </div>
    <p class="text-xs text-muted">
      Mint a dedicated Group/Project Access Token (scopes <code>api</code> +
      <code>write_repository</code>, Maintainer role) so the agent acts as its own bot.
      The service's current token is used once as the owner-scoped bootstrap, then replaced
      by the minted token.
    </p>

    <p v-if="gitlabToken" class="text-xs text-muted">
      Current: <span class="font-mono">{{ gitlabToken.scope }}</span> token #{{ gitlabToken.token_id }}
      on <span class="font-mono">{{ gitlabToken.namespace }}</span
      ><template v-if="gitlabToken.expires_at">, expires {{ gitlabToken.expires_at }}</template>.
    </p>

    <div class="grid grid-cols-2 gap-3">
      <div>
        <label class="label">Scope</label>
        <select v-model="provScope" class="select">
          <option value="group">Group</option>
          <option value="project">Project</option>
        </select>
      </div>
      <div>
        <label class="label">Group / project path or id</label>
        <input v-model="provNamespace" class="input font-mono" placeholder="my-group/sub" />
      </div>
      <div>
        <label class="label">Token name <span class="text-faint">(optional)</span></label>
        <input v-model="provName" class="input font-mono" :placeholder="`agent-${slug}`" />
      </div>
      <div>
        <label class="label">Expires <span class="text-faint">(optional, ≤365d)</span></label>
        <input v-model="provExpiry" type="date" class="input font-mono" />
      </div>
    </div>

    <div class="flex gap-2">
      <button type="button" class="btn btn-primary" :disabled="provisioning" @click="provisionToken">
        {{ provisioning ? "Minting…" : gitlabToken ? "Re-provision" : "Provision token" }}
      </button>
      <button
        v-if="gitlabToken"
        type="button"
        class="btn btn-ghost"
        :disabled="rotating"
        @click="rotateToken"
      >
        {{ rotating ? "Rotating…" : "Rotate" }}
      </button>
    </div>
  </section>
</template>
