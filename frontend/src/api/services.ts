import { api } from "./client";
import type {
  GitHubAppSyncResult,
  ProvisionGitLabToken,
  ServiceView,
  NewService,
  UpdateService,
} from "../types/api";

export const servicesApi = {
  list(): Promise<ServiceView[]> {
    return api("/api/services");
  },
  get(id: string): Promise<ServiceView> {
    return api(`/api/services/${id}`);
  },
  create(body: NewService): Promise<ServiceView> {
    return api("/api/services", { method: "POST", body });
  },
  update(id: string, body: UpdateService): Promise<ServiceView> {
    return api(`/api/services/${id}`, { method: "PUT", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/services/${id}`, { method: "DELETE" });
  },
  githubAppInstallUrl(id: string): Promise<{ install_url: string }> {
    return api(`/api/services/${id}/github_app/install`);
  },
  githubAppSync(id: string): Promise<GitHubAppSyncResult> {
    return api(`/api/services/${id}/github_app/sync`, { method: "POST" });
  },
  provisionGitlabToken(
    id: string,
    body: ProvisionGitLabToken,
  ): Promise<ServiceView> {
    return api(`/api/services/${id}/gitlab_token/provision`, {
      method: "POST",
      body,
    });
  },
  rotateGitlabToken(id: string): Promise<ServiceView> {
    return api(`/api/services/${id}/gitlab_token/rotate`, { method: "POST" });
  },
};
