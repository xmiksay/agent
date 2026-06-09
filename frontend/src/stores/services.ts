import { defineStore } from "pinia";
import { ref } from "vue";
import { servicesApi } from "../api/services";
import type {
  ServiceView,
  NewService,
  UpdateService,
} from "../types/api";

export const useServicesStore = defineStore("services", () => {
  const list = ref<ServiceView[]>([]);
  const detail = ref<ServiceView | null>(null);
  const loading = ref(false);

  async function refresh() {
    loading.value = true;
    try {
      list.value = await servicesApi.list();
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await servicesApi.get(id);
  }

  async function create(body: NewService) {
    const created = await servicesApi.create(body);
    list.value = [...list.value, created];
    return created;
  }

  async function update(id: string, body: UpdateService) {
    const updated = await servicesApi.update(id, body);
    list.value = list.value.map((s) => (s.id === id ? updated : s));
    if (detail.value?.id === id) detail.value = updated;
    return updated;
  }

  async function remove(id: string) {
    await servicesApi.remove(id);
    list.value = list.value.filter((s) => s.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return { list, detail, loading, refresh, load, create, update, remove };
});
