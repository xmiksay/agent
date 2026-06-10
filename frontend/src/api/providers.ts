import { api } from "./client";
import type {
  ProvidersListResponse,
  ProviderView,
  NewProvider,
  UpdateProvider,
} from "../types/api";

export const providersApi = {
  list(): Promise<ProvidersListResponse> {
    return api("/api/providers");
  },
  get(id: string): Promise<ProviderView> {
    return api(`/api/providers/${id}`);
  },
  create(body: NewProvider): Promise<ProviderView> {
    return api("/api/providers", { method: "POST", body });
  },
  update(id: string, body: UpdateProvider): Promise<ProviderView> {
    return api(`/api/providers/${id}`, { method: "PUT", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/providers/${id}`, { method: "DELETE" });
  },
};
