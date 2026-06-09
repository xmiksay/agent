import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { modelProvidersApi } from "../api/model_providers";
import type {
  ModelProviderView,
  NewModelProvider,
  UpdateModelProvider,
} from "../types/api";

export const useModelProvidersStore = defineStore("model_providers", () => {
  const list = ref<ModelProviderView[]>([]);
  const kinds = ref<string[]>([]);
  const detail = ref<ModelProviderView | null>(null);
  const loading = ref(false);

  // { value: id, label: name } pairs for binding into the model form's
  // provider <select>.
  const options = computed(() =>
    list.value.map((p) => ({ value: p.id, label: p.name })),
  );

  async function refresh() {
    loading.value = true;
    try {
      const res = await modelProvidersApi.list();
      list.value = res.providers;
      kinds.value = res.kinds;
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await modelProvidersApi.get(id);
  }

  async function create(body: NewModelProvider) {
    const created = await modelProvidersApi.create(body);
    list.value = [...list.value, created];
    return created;
  }

  async function update(id: string, body: UpdateModelProvider) {
    const updated = await modelProvidersApi.update(id, body);
    list.value = list.value.map((p) => (p.id === id ? updated : p));
    if (detail.value?.id === id) detail.value = updated;
    return updated;
  }

  async function remove(id: string) {
    await modelProvidersApi.remove(id);
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
