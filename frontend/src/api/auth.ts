import { api } from "./client";
import type { AuthRequest } from "../types/api";

export interface AuthListParams {
  status?: string;
  task_id?: string;
}

export const authApi = {
  list(params?: AuthListParams): Promise<AuthRequest[]> {
    return api("/api/auth_requests", { params });
  },
  get(id: string): Promise<AuthRequest> {
    return api(`/api/auth_requests/${id}`);
  },
  resolve(id: string, decision: "approve" | "deny", reply?: string): Promise<AuthRequest> {
    return api(`/api/auth_requests/${id}/resolve`, {
      method: "POST",
      body: { decision, reply: reply ?? null },
    });
  },
};
