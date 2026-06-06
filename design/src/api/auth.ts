// Workbench stub of the live auth API. Same shape as agent/frontend/src/api/auth.ts
// so AuthApprovalForm / InlineAuthApproval port unchanged — here it just echoes
// an optimistic resolution instead of hitting the backend.
import type { AuthRequest } from "../types/api";

export const authApi = {
  async list(_params?: { task_id?: string; status?: string }): Promise<AuthRequest[]> {
    return [];
  },
  async resolve(
    id: string,
    decision: "approve" | "deny",
    body?: string,
  ): Promise<AuthRequest> {
    return {
      id,
      task_id: "demo",
      requested_op: "",
      prompt_to_operator: "",
      status: decision === "approve" ? "approved" : "denied",
      operator_reply: body ?? null,
      created_at: new Date(0).toISOString(),
      resolved_at: new Date(0).toISOString(),
    };
  },
};
