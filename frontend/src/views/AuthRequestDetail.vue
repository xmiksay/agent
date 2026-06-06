<script setup lang="ts">
import { onMounted } from "vue";
import { useAuthRequestsStore } from "../stores/authRequests";
import StatusPill from "../components/StatusPill.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import type { AuthRequest } from "../types/api";

const props = defineProps<{ id: string }>();
const store = useAuthRequestsStore();

onMounted(() => store.load(props.id));

function onResolved(updated: AuthRequest) {
  if (store.detail && store.detail.id === updated.id) store.detail = updated;
}
</script>

<template>
  <section v-if="store.detail" class="space-y-4">
    <RouterLink to="/auth_requests" class="inline-block text-sm text-muted hover:text-accent">
      ← Auth queue
    </RouterLink>

    <header class="flex items-center gap-3">
      <h1 class="font-display font-mono text-xl font-bold">{{ store.detail.id }}</h1>
      <StatusPill :status="store.detail.status" />
      <RouterLink
        :to="`/tasks/${store.detail.task_id}`"
        class="ml-auto font-mono text-xs text-accent hover:underline"
      >
        task {{ store.detail.task_id }} →
      </RouterLink>
    </header>

    <div class="card space-y-3 p-5">
      <div>
        <h2 class="label mb-1">Requested operation</h2>
        <pre class="whitespace-pre-wrap rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-ink">{{ store.detail.requested_op }}</pre>
      </div>
      <div>
        <h2 class="label mb-1">Prompt</h2>
        <p class="whitespace-pre-wrap text-sm text-muted">{{ store.detail.prompt_to_operator }}</p>
      </div>

      <AuthApprovalForm
        v-if="store.detail.status === 'pending'"
        :item="store.detail"
        @resolved="onResolved"
      />

      <div v-if="store.detail.operator_reply">
        <h2 class="label mb-1">Operator reply</h2>
        <pre class="whitespace-pre-wrap rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-muted">{{ store.detail.operator_reply }}</pre>
      </div>
    </div>
  </section>
  <p v-else class="text-faint">Loading…</p>
</template>
