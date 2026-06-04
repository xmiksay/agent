import { defineStore } from "pinia";
import { ref } from "vue";
import { gitServicesApi } from "../api/git_services";
import type {
  GitServiceView,
  NewGitService,
  UpdateGitService,
} from "../types/api";

export const useGitServicesStore = defineStore("git_services", () => {
  const list = ref<GitServiceView[]>([]);
  const detail = ref<GitServiceView | null>(null);
  const loading = ref(false);

  async function refresh() {
    loading.value = true;
    try {
      list.value = await gitServicesApi.list();
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await gitServicesApi.get(id);
  }

  async function create(body: NewGitService) {
    const created = await gitServicesApi.create(body);
    list.value = [...list.value, created];
    return created;
  }

  async function update(id: string, body: UpdateGitService) {
    const updated = await gitServicesApi.update(id, body);
    list.value = list.value.map((s) => (s.id === id ? updated : s));
    if (detail.value?.id === id) detail.value = updated;
    return updated;
  }

  async function remove(id: string) {
    await gitServicesApi.remove(id);
    list.value = list.value.filter((s) => s.id !== id);
    if (detail.value?.id === id) detail.value = null;
  }

  return { list, detail, loading, refresh, load, create, update, remove };
});
