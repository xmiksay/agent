import { api } from "./client";
import type { Task, TaskDetail, TaskOutput } from "../types/api";

export const tasksApi = {
  list(status?: string): Promise<Task[]> {
    return api("/api/tasks", { params: status ? { status } : undefined });
  },
  get(id: string): Promise<TaskDetail> {
    return api(`/api/tasks/${id}`);
  },
  confirm(id: string): Promise<void> {
    return api(`/api/tasks/${id}/confirm`, { method: "POST" });
  },
  retry(id: string): Promise<{ task_id: string }> {
    return api(`/api/tasks/${id}/retry`, { method: "POST" });
  },
  kill(id: string): Promise<void> {
    return api(`/api/tasks/${id}/kill`, { method: "POST" });
  },
  continue_(id: string): Promise<{ task_id: string }> {
    return api(`/api/tasks/${id}/continue`, { method: "POST" });
  },
  remove(id: string): Promise<void> {
    return api(`/api/tasks/${id}`, { method: "DELETE" });
  },
  output(id: string): Promise<TaskOutput> {
    return api(`/api/tasks/${id}/output`);
  },
  pushMessage(id: string, body: string): Promise<void> {
    return api(`/api/tasks/${id}/message`, {
      method: "POST",
      body: { body },
    });
  },
  diff(id: string): Promise<{ diff: string }> {
    return api(`/api/tasks/${id}/diff`);
  },
};
