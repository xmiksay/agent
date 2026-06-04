import { api } from "./client";
import type {
  GitServiceView,
  NewGitService,
  UpdateGitService,
} from "../types/api";

export const gitServicesApi = {
  list(): Promise<GitServiceView[]> {
    return api("/api/git_services");
  },
  get(id: string): Promise<GitServiceView> {
    return api(`/api/git_services/${id}`);
  },
  create(body: NewGitService): Promise<GitServiceView> {
    return api("/api/git_services", { method: "POST", body });
  },
  update(id: string, body: UpdateGitService): Promise<GitServiceView> {
    return api(`/api/git_services/${id}`, { method: "PUT", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/git_services/${id}`, { method: "DELETE" });
  },
};
