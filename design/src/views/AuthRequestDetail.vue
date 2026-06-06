<script setup lang="ts">
import { ref } from "vue";
import { RouterLink } from "vue-router";
import StatusPill from "../components/StatusPill.vue";
import AuthApprovalForm from "../components/AuthApprovalForm.vue";
import { authRequests } from "../fixtures";
import type { AuthRequest } from "../types/api";

const props = defineProps<{ id: string }>();
const detail = ref<AuthRequest | undefined>(authRequests.find((a) => a.id === props.id));
function onResolved(r: AuthRequest) {
  if (detail.value) detail.value = { ...detail.value, status: r.status, operator_reply: r.operator_reply };
}
</script>

<template>
  <section v-if="detail" class="space-y-4">
    <RouterLink to="/auth_requests" class="inline-block text-sm text-muted hover:text-accent">← Auth queue</RouterLink>

    <header class="flex items-center gap-3">
      <h1 class="font-display text-xl font-bold font-mono">{{ detail.id }}</h1>
      <StatusPill :status="detail.status" />
      <RouterLink :to="`/tasks/${detail.task_id}`" class="ml-auto font-mono text-xs text-accent hover:underline">
        task {{ detail.task_id }} →
      </RouterLink>
    </header>

    <div class="card space-y-3 p-5">
      <div>
        <h2 class="label mb-1">Requested operation</h2>
        <pre class="rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-ink whitespace-pre-wrap">{{ detail.requested_op }}</pre>
      </div>
      <div>
        <h2 class="label mb-1">Prompt</h2>
        <p class="whitespace-pre-wrap text-sm text-muted">{{ detail.prompt_to_operator }}</p>
      </div>

      <AuthApprovalForm v-if="detail.status === 'pending'" :item="detail" @resolved="onResolved" />

      <div v-if="detail.operator_reply">
        <h2 class="label mb-1">Operator reply</h2>
        <pre class="rounded-md border border-line bg-canvas px-3 py-2 font-mono text-xs text-muted whitespace-pre-wrap">{{ detail.operator_reply }}</pre>
      </div>
    </div>
  </section>
  <p v-else class="text-faint">Auth request not found.</p>
</template>
