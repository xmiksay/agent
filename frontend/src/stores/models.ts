import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { modelsApi } from "../api/models";
import type { AiModel, NewModel, UpdateModel } from "../types/api";

export const useModelsStore = defineStore("models", () => {
  const list = ref<AiModel[]>([]);
  const detail = ref<AiModel | null>(null);
  const loading = ref(false);

  // { value: id, label: alias } pairs for binding into <select> menus. Unbound
  // (dangerous) models get a ⚠ … (unbound) label so the foot-gun is visible in
  // every dropdown; `unbound` is exposed too for richer rendering.
  const options = computed(() =>
    list.value.map((m) => ({
      value: m.id,
      label: m.unbound ? `⚠ ${m.alias} (unbound)` : m.alias,
      unbound: m.unbound,
    })),
  );

  async function refresh() {
    loading.value = true;
    try {
      list.value = await modelsApi.list();
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await modelsApi.get(id);
  }

  async function create(body: NewModel) {
    const created = await modelsApi.create(body);
    // A new default demotes any prior one server-side; refetch to stay in sync.
    if (created.is_default) await refresh();
    else list.value = [...list.value, created];
    return created;
  }

  async function update(id: string, body: UpdateModel) {
    const updated = await modelsApi.update(id, body);
    // Promoting a default demotes the others server-side; refetch to reflect it.
    if (updated.is_default) await refresh();
    else list.value = list.value.map((m) => (m.id === id ? updated : m));
    if (detail.value?.id === id) detail.value = updated;
    return updated;
  }

  async function remove(id: string) {
    await modelsApi.remove(id);
    list.value = list.value.filter((m) => m.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return {
    list,
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
