<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useAuthRequestsStore } from "../stores/authRequests";
import StatusPill from "../components/StatusPill.vue";

const store = useAuthRequestsStore();
const replies = ref<Record<string, string>>({});
let timer: number | null = null;

async function reload() {
  await store.refresh("pending");
}

onMounted(() => {
  reload();
  timer = window.setInterval(reload, 3000);
});

onUnmounted(() => {
  if (timer !== null) window.clearInterval(timer);
});

async function decide(id: string, decision: "approve" | "deny") {
  await store.resolve(id, decision, replies.value[id] || undefined);
  delete replies.value[id];
  reload();
}
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold mb-4">Pending operator approvals</h1>
    <ul class="space-y-3">
      <li v-for="r in store.list" :key="r.id" class="bg-white rounded shadow-sm p-4 space-y-2">
        <div class="flex items-center gap-2 text-xs text-gray-500">
          <span class="font-mono">{{ r.id.slice(0, 8) }}</span>
          <StatusPill :status="r.status" />
          <span>{{ new Date(r.created_at).toLocaleTimeString() }}</span>
          <RouterLink :to="`/tasks/${r.task_id}`" class="ml-auto">task</RouterLink>
        </div>
        <pre class="bg-ink-50 rounded text-xs p-2 whitespace-pre-wrap">{{ r.requested_op }}</pre>
        <p class="text-sm text-gray-700">{{ r.prompt_to_operator }}</p>
        <textarea
          v-model="replies[r.id]"
          rows="2"
          placeholder="Optional reply forwarded to Claude…"
          class="w-full border rounded p-2 text-sm"
        />
        <div class="flex gap-2">
          <button
            class="rounded bg-green-600 text-white px-3 py-1 hover:bg-green-700"
            @click="decide(r.id, 'approve')"
          >
            Approve
          </button>
          <button
            class="rounded bg-red-600 text-white px-3 py-1 hover:bg-red-700"
            @click="decide(r.id, 'deny')"
          >
            Deny
          </button>
        </div>
      </li>
      <li v-if="!store.list.length" class="text-gray-500 text-sm">No pending requests.</li>
    </ul>
  </section>
</template>
