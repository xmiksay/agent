import { ofetch, type FetchOptions } from "ofetch";
import { useSessionStore } from "../stores/session";

export const api = ofetch.create({
  baseURL: "",
  onRequest({ options }) {
    const session = useSessionStore();
    if (session.hasToken) {
      const headers = new Headers((options.headers ?? {}) as HeadersInit);
      headers.set("Authorization", `Bearer ${session.token}`);
      options.headers = headers;
    }
  },
  onResponse({ response }) {
    if (response.status >= 200 && response.status < 300) {
      const session = useSessionStore();
      if (session.hasToken && session.validated !== true) {
        session.markValid();
      }
    }
  },
  onResponseError({ response }) {
    if (response?.status === 401) {
      const session = useSessionStore();
      session.markInvalid();
    } else {
      console.error("API error", response?.status, response?._data);
    }
  },
});

/** Probe whether the current token is accepted. */
export async function probeAuth(): Promise<boolean> {
  try {
    await api("/api/auth/check", { method: "GET" } as FetchOptions);
    return true;
  } catch {
    return false;
  }
}
