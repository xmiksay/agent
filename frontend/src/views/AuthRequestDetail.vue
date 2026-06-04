<script setup lang="ts">
import { onMounted } from "vue";
import { useAuthRequestsStore } from "../stores/authRequests";
import StatusPill from "../components/StatusPill.vue";

const props = defineProps<{ id: string }>();
const store = useAuthRequestsStore();

onMounted(() => store.load(props.id));
</script>

<template>
  <section v-if="store.detail" class="space-y-3">
    <header class="flex items-center gap-3">
      <h1 class="text-xl font-semibold font-mono">{{ store.detail.id }}</h1>
      <StatusPill :status="store.detail.status" />
    </header>
    <p>
      <RouterLink :to="`/tasks/${store.detail.task_id}`">
        ← task {{ store.detail.task_id }}
      </RouterLink>
    </p>
    <h2 class="font-medium mt-3">Requested operation</h2>
    <pre class="bg-ink-50 p-2 rounded text-xs whitespace-pre-wrap">{{ store.detail.requested_op }}</pre>
    <h2 class="font-medium">Prompt</h2>
    <p class="text-sm">{{ store.detail.prompt_to_operator }}</p>
    <template v-if="store.detail.operator_reply">
      <h2 class="font-medium">Operator reply</h2>
      <pre class="bg-ink-50 p-2 rounded text-xs whitespace-pre-wrap">{{ store.detail.operator_reply }}</pre>
    </template>
  </section>
  <p v-else class="text-gray-500">Loading…</p>
</template>
