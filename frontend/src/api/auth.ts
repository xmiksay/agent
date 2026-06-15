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
  // `answers` carries structured AskUserQuestion replies — a map of question
  // text to the chosen label(s) or a custom string. The backend stringifies it
  // into operator_reply for the parked question handler; `reply` stays the
  // freeform path for non-question (Bash) approvals.
  resolve(
    id: string,
    decision: "approve" | "deny",
    reply?: string,
    answers?: Record<string, string | string[]>,
  ): Promise<AuthRequest> {
    return api(`/api/auth_requests/${id}/resolve`, {
      method: "POST",
      body: { decision, reply: reply ?? null, answers: answers ?? null },
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
