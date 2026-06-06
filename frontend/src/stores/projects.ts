import { defineStore } from "pinia";
import { ref } from "vue";
import { projectsApi } from "../api/projects";
import type { ProjectDetailResponse, ProjectListItem } from "../types/api";

export const useProjectsStore = defineStore("projects", () => {
  const list = ref<ProjectListItem[]>([]);
  const detail = ref<ProjectDetailResponse | null>(null);
  const loading = ref(false);

  async function refresh() {
    loading.value = true;
    try {
      list.value = await projectsApi.list();
    } finally {
      loading.value = false;
    }
  }

  async function load(id: string) {
    detail.value = await projectsApi.get(id);
  }

  async function updateAllowedOps(id: string, ops: string[]) {
    const updated = await projectsApi.updateConfig(id, {
      allowed_operations: ops,
    });
    if (detail.value && detail.value.id === updated.id) {
      detail.value = { ...detail.value, ...updated };
    }
    return updated;
  }

  async function updateEnvFile(id: string, envFile: string) {
    const updated = await projectsApi.updateEnv(id, { env_file: envFile });
    if (detail.value && detail.value.id === updated.id) {
      detail.value = { ...detail.value, ...updated };
    }
    return updated;
  }

  return {
    list,
    detail,
    loading,
    refresh,
    load,
    updateAllowedOps,
    updateEnvFile,
  };
});
