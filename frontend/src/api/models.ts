import { api } from "./client";
import type { AiModel, NewModel, UpdateModel } from "../types/api";

export const modelsApi = {
  list(): Promise<AiModel[]> {
    return api("/api/models");
  },
  get(id: string): Promise<AiModel> {
    return api(`/api/models/${id}`);
  },
  create(body: NewModel): Promise<AiModel> {
    return api("/api/models", { method: "POST", body });
  },
  update(id: string, body: UpdateModel): Promise<AiModel> {
    return api(`/api/models/${id}`, { method: "PUT", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/models/${id}`, { method: "DELETE" });
  },
};
