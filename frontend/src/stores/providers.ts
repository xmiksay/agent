import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { providersApi } from "../api/providers";
import type {
  ProviderView,
  NewProvider,
  UpdateProvider,
} from "../types/api";

export const useProvidersStore = defineStore("providers", () => {
  const list = ref<ProviderView[]>([]);
  const kinds = ref<string[]>([]);
  const detail = ref<ProviderView | null>(null);
  const loading = ref(false);

  // { value: id, label: name } pairs for binding into the model form's
  // provider <select>.
  const options = computed(() =>
    list.value.map((p) => ({ value: p.id, label: p.name })),
  );

  async function refresh() {
    loading.value = true;
    try {
      const res = await providersApi.list();
      list.value = res.providers;
      kinds.value = res.kinds;
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await providersApi.get(id);
  }

  async function create(body: NewProvider) {
    const created = await providersApi.create(body);
    list.value = [...list.value, created];
    return created;
  }

  async function update(id: string, body: UpdateProvider) {
    const updated = await providersApi.update(id, body);
    list.value = list.value.map((p) => (p.id === id ? updated : p));
    if (detail.value?.id === id) detail.value = updated;
    return updated;
  }

  async function remove(id: string) {
    await providersApi.remove(id);
    list.value = list.value.filter((p) => p.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return {
    list,
    kinds,
    detail,
    loading,
    options,
    refresh,
    load,
    create,
    update,
    remove,
  };
});
