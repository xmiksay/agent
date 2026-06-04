import { defineStore } from "pinia";
import { ref } from "vue";
import { authApi } from "../api/auth";
import type { AuthRequest } from "../types/api";

export const useAuthRequestsStore = defineStore("authRequests", () => {
  const list = ref<AuthRequest[]>([]);
  const detail = ref<AuthRequest | null>(null);

  async function refresh(status?: string) {
    list.value = await authApi.list({ status });
  }

  async function load(id: string) {
    detail.value = await authApi.get(id);
  }

  async function resolve(id: string, decision: "approve" | "deny", reply?: string) {
    const updated = await authApi.resolve(id, decision, reply);
    if (detail.value && detail.value.id === id) detail.value = updated;
    return updated;
  }

  return { list, detail, refresh, load, resolve };
});
