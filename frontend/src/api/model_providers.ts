import { api } from "./client";
import type {
  ModelProvidersListResponse,
  ModelProviderView,
  NewModelProvider,
  UpdateModelProvider,
} from "../types/api";

export const modelProvidersApi = {
  list(): Promise<ModelProvidersListResponse> {
    return api("/api/model_providers");
  },
  get(id: string): Promise<ModelProviderView> {
    return api(`/api/model_providers/${id}`);
  },
  create(body: NewModelProvider): Promise<ModelProviderView> {
    return api("/api/model_providers", { method: "POST", body });
  },
  update(id: string, body: UpdateModelProvider): Promise<ModelProviderView> {
    return api(`/api/model_providers/${id}`, { method: "PUT", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/model_providers/${id}`, { method: "DELETE" });
  },
};
