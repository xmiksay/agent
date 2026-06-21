import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { queuesApi } from "../api/queues";
import type { Queue } from "../types/api";

export const useQueuesStore = defineStore("queues", () => {
  const list = ref<Queue[]>([]);
  const loading = ref(false);

  // { value: id, label: name } pairs for binding into a queue <select>.
  const options = computed(() =>
    list.value.map((q) => ({ value: q.id, label: q.name })),
  );

  // Map a queue id to its display name; null/unknown ids resolve to null so
  // callers can fall back gracefully.
  function nameFor(id: string | null | undefined): string | null {
    if (!id) return null;
    return list.value.find((q) => q.id === id)?.name ?? null;
  }

  async function refresh() {
    loading.value = true;
    try {
      list.value = await queuesApi.list();
    } finally {
      loading.value = false;
    }
  }

  return { list, loading, options, nameFor, refresh };
});
