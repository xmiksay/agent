import { api } from "./client";
import type { NewQueue, Queue, UpdateQueue } from "../types/api";

export const queuesApi = {
  list(): Promise<Queue[]> {
    return api("/api/queues");
  },
  get(id: string): Promise<Queue> {
    return api(`/api/queues/${id}`);
  },
  create(body: NewQueue): Promise<Queue> {
    return api("/api/queues", { method: "POST", body });
  },
  update(id: string, body: UpdateQueue): Promise<Queue> {
    return api(`/api/queues/${id}`, { method: "PATCH", body });
  },
  remove(id: string): Promise<void> {
    return api(`/api/queues/${id}`, { method: "DELETE" });
  },
};
