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
    <h1 class="text-2xl font-semibold mb-4">Pending operator approvals</h1>
    <ul class="space-y-3">
      <li v-for="r in pending" :key="r.id" class="bg-white rounded shadow-sm p-4 space-y-2">
        <div class="flex items-center gap-2 text-xs text-gray-500">
          <span class="font-mono">{{ r.id.slice(0, 8) }}</span>
          <StatusPill :status="r.status" />
          <span>{{ new Date(r.created_at).toLocaleTimeString() }}</span>
          <RouterLink :to="`/tasks/${r.task_id}`" class="ml-auto">task</RouterLink>
        </div>
        <pre class="bg-ink-50 rounded text-xs p-2 whitespace-pre-wrap">{{ r.requested_op }}</pre>
        <p class="text-sm text-gray-700">{{ r.prompt_to_operator }}</p>
        <AuthApprovalForm :item="r" compact @resolved="onResolved" />
      </li>
      <li v-if="!pending.length" class="text-gray-500 text-sm">No pending requests.</li>
    </ul>
  </section>
</template>
