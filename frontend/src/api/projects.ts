import { api } from "./client";
import type {
  BranchEntry,
  ProjectConfig,
  ProjectDetailResponse,
  ProjectListItem,
} from "../types/api";

export const projectsApi = {
  list(): Promise<ProjectListItem[]> {
    return api("/api/projects");
  },
  get(id: string): Promise<ProjectDetailResponse> {
    return api(`/api/projects/${id}`);
  },
  branches(id: string): Promise<BranchEntry[]> {
    return api(`/api/projects/${id}/branches`);
  },
  updateConfig(
    id: string,
    body: { allowed_operations: string[] },
  ): Promise<ProjectConfig> {
    return api(`/api/projects/${id}/config`, {
      method: "PUT",
      body,
    });
  },
  updateEnv(id: string, body: { env_file: string }): Promise<ProjectConfig> {
    return api(`/api/projects/${id}/env`, {
      method: "PUT",
      body,
    });
  },
};
