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
  // Resolve many at once. Pass `all_pending` to target every pending request, or
  // an explicit `ids` list. Returns how many rows were resolved.
  bulkResolve(
    opts: { ids?: string[]; all_pending?: boolean; decision: "approve" | "deny"; reply?: string },
  ): Promise<{ resolved: number }> {
    return api("/api/auth_requests/bulk_resolve", {
      method: "POST",
      body: {
        ids: opts.ids ?? [],
        all_pending: opts.all_pending ?? false,
        decision: opts.decision,
        reply: opts.reply ?? null,
      },
    });
  },
};
