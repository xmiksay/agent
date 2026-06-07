import { api } from "./client";
import type {
  NewTaskBody,
  PersistedEvent,
  StatsQuery,
  StatsResponse,
  Task,
  TaskDetail,
  TaskEdits,
} from "../types/api";

export const tasksApi = {
  list(status?: string): Promise<Task[]> {
    // `no-store`: the task list is a live view, so never serve a stale cached
    // response — every poll must hit the server.
    return api("/api/tasks", {
      params: status ? { status } : undefined,
      cache: "no-store",
    });
  },
  create(body: NewTaskBody): Promise<{ task_id: string }> {
    return api("/api/tasks", { method: "POST", body });
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
  events(id: string): Promise<{ events: PersistedEvent[] }> {
    return api(`/api/tasks/${id}/events`);
  },
  pushMessage(id: string, body: string): Promise<void> {
    return api(`/api/tasks/${id}/message`, {
      method: "POST",
      body: { body },
    });
  },
  update(id: string, edits: TaskEdits): Promise<void> {
    return api(`/api/tasks/${id}`, {
      method: "PATCH",
      body: edits,
    });
  },
  diff(id: string): Promise<{ diff: string }> {
    return api(`/api/tasks/${id}/diff`);
  },
  stats(query: StatsQuery): Promise<StatsResponse> {
    const params: Record<string, string> = {};
    if (query.from) params.from = query.from;
    if (query.to) params.to = query.to;
    if (query.group_by) params.group_by = query.group_by;
    return api("/api/tasks/stats", { params });
  },
};
