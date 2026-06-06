<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useStreamStore } from "../stores/stream";
import { authApi } from "../api/auth";
import StatusPill from "../components/StatusPill.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import type { AuthRequest } from "../types/api";

// Approvals stream in live over the single app-wide socket (the same store that
// powers the nav badge). Seed once from REST to cover any raised before this
// client connected — no polling.
const stream = useStreamStore();

onMounted(async () => {
  try {
    stream.seedApprovals(await authApi.list({ status: "pending" }));
  } catch {
    /* ignore — list just stays as whatever the socket has delivered */
  }
});

const pending = computed<AuthRequest[]>(() =>
  [...stream.approvals.values()]
    .filter((a) => a.status === "pending")
    .sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime()),
);

function onResolved(r: AuthRequest) {
  stream.dropApproval(r.id);
}
</script>

<template>
  <section>
    <div class="mb-6 flex items-center gap-2">
      <h1 class="font-display text-2xl font-bold tracking-tight">Pending operator approvals</h1>
      <span v-if="pending.length" class="pill text-accent">
        <span class="led text-accent" /> {{ pending.length }} pending
      </span>
    </div>

    <div v-if="!pending.length" class="card p-10 text-center text-faint">
      <span class="led mx-auto mb-2 block w-fit text-signal-ok" />
      No pending requests.
    </div>

    <ul v-else class="space-y-3">
      <li v-for="r in pending" :key="r.id" class="card space-y-2 border-accent/30 p-4">
        <div class="flex items-center gap-2 text-xs text-faint">
          <span class="font-mono">{{ r.id.slice(0, 8) }}</span>
          <StatusPill :status="r.status" />
          <span class="font-mono">{{ new Date(r.created_at).toLocaleTimeString() }}</span>
          <RouterLink :to="`/tasks/${r.task_id}`" class="ml-auto">task</RouterLink>
        </div>
        <pre class="whitespace-pre-wrap rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-ink">{{ r.requested_op }}</pre>
        <p class="text-sm text-muted">{{ r.prompt_to_operator }}</p>
        <AuthApprovalForm :item="r" compact @resolved="onResolved" />
      </li>
    </ul>
  </section>
</template>
