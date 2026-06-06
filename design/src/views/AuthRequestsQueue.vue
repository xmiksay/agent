<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import { authRequests } from "../fixtures";
import type { AuthRequest } from "../types/api";

const router = useRouter();
const items = ref<AuthRequest[]>(authRequests.filter((a) => a.status === "pending"));
function onResolved(r: AuthRequest) {
  items.value = items.value.filter((i) => i.id !== r.id);
}
</script>

<template>
  <section>
    <div class="mb-6 flex items-center gap-2">
      <h1 class="font-display text-2xl font-bold tracking-tight">Auth queue</h1>
      <span v-if="items.length" class="pill text-accent"><span class="led text-accent" /> {{ items.length }} pending</span>
    </div>

    <div v-if="!items.length" class="card p-10 text-center text-faint">
      <span class="led mx-auto mb-2 block w-fit text-signal-ok" />
      Nothing waiting on you.
    </div>

    <ul v-else class="space-y-3">
      <li
        v-for="item in items"
        :key="item.id"
        class="card cursor-pointer space-y-3 border-accent/30 p-4 transition-colors hover:border-accent/60"
        @click="router.push(`/auth_requests/${item.id}`)"
      >
        <div class="flex items-center gap-2">
          <span class="led text-signal-auth" />
          <span class="font-mono text-xs text-muted">task {{ item.task_id }}</span>
          <span class="ml-auto font-mono text-[11px] text-faint">{{ new Date(item.created_at).toLocaleString() }}</span>
        </div>
        <pre class="rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-ink whitespace-pre-wrap">{{ item.requested_op }}</pre>
        <p class="text-sm text-muted">{{ item.prompt_to_operator }}</p>
        <!-- Controls live inside the clickable card; stop clicks so approving doesn't navigate. -->
        <div @click.stop>
          <AuthApprovalForm :item="item" @resolved="onResolved" />
        </div>
      </li>
    </ul>
  </section>
</template>
