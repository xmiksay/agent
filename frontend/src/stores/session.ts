import { defineStore } from "pinia";
import { computed, ref } from "vue";

const STORAGE_KEY = "agent_bearer_token";

export const useSessionStore = defineStore("session", () => {
  const token = ref<string | null>(localStorage.getItem(STORAGE_KEY));
  // null = unchecked, true = ok, false = rejected
  const validated = ref<boolean | null>(null);

  const hasToken = computed(() => (token.value ?? "").length > 0);
  const authHeaders = computed<Record<string, string>>(() => {
    const out: Record<string, string> = {};
    if (hasToken.value) out.Authorization = `Bearer ${token.value}`;
    return out;
  });

  function set(value: string) {
    const trimmed = value.trim();
    token.value = trimmed || null;
    if (trimmed) localStorage.setItem(STORAGE_KEY, trimmed);
    else localStorage.removeItem(STORAGE_KEY);
    validated.value = null;
  }

  function clear() {
    token.value = null;
    localStorage.removeItem(STORAGE_KEY);
    validated.value = false;
  }

  function markValid() {
    validated.value = true;
  }

  function markInvalid() {
    validated.value = false;
  }

  return { token, hasToken, validated, authHeaders, set, clear, markValid, markInvalid };
});
