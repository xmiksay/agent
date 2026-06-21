import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useStreamStore } from "./stream";
import type { AuthRequest, PersistedEvent, StreamEnvelope } from "../types/api";

function authRequest(over: Partial<AuthRequest> = {}): AuthRequest {
  return {
    id: "a1",
    task_id: "t1",
    requested_op: "ls",
    prompt_to_operator: "Run ls?",
    status: "pending",
    operator_reply: null,
    created_at: "2026-01-01T00:00:00Z",
    resolved_at: null,
    ...over,
  };
}

function envelope(over: Partial<StreamEnvelope> = {}): StreamEnvelope {
  return { task_id: "t1", agent: "claude", seq: 0, kind: "event", payload: {}, ...over };
}

describe("useStreamStore", () => {
  beforeEach(() => setActivePinia(createPinia()));

  describe("seedEvents", () => {
    it("replays only `event` frames, keyed by seq", () => {
      const store = useStreamStore();
      const items: PersistedEvent[] = [
        { seq: 0, kind: "event", payload: { a: 1 } },
        { seq: 1, kind: "event", payload: { a: 2 } },
        { seq: 2, kind: "status", payload: { task_state: "completed", agent_state: "cold" } },
        { seq: 3, kind: "auth_request", payload: authRequest() },
      ];
      store.seedEvents("t1", items);

      const m = store.eventsFor("t1")!;
      expect(m.size).toBe(2);
      expect(m.get(0)).toEqual({ a: 1 });
      // Control frames are intentionally not seeded.
      expect(store.approvals.size).toBe(0);
      expect(store.statusByTask.size).toBe(0);
    });

    it("dedupes a live frame against a seeded frame with the same seq", () => {
      const store = useStreamStore();
      store.seedEvents("t1", [{ seq: 5, kind: "event", payload: { v: "old" } }]);
      store.apply(envelope({ seq: 5, payload: { v: "new" } }));

      const m = store.eventsFor("t1")!;
      expect(m.size).toBe(1);
      expect(m.get(5)).toEqual({ v: "new" });
    });
  });

  describe("approval lifecycle", () => {
    it("seedApprovals adds pending requests", () => {
      const store = useStreamStore();
      store.seedApprovals([authRequest({ id: "a1" }), authRequest({ id: "a2" })]);
      expect(store.approvals.size).toBe(2);
      expect(store.approvals.get("a1")?.id).toBe("a1");
    });

    it("adds a pending auth_request frame and removes it once resolved", () => {
      const store = useStreamStore();
      store.apply(envelope({ kind: "auth_request", payload: authRequest({ id: "a1", status: "pending" }) }));
      expect(store.approvals.has("a1")).toBe(true);

      store.apply(envelope({ kind: "auth_request", payload: authRequest({ id: "a1", status: "approved" }) }));
      expect(store.approvals.has("a1")).toBe(false);
    });

    it("dropApproval removes a request by id", () => {
      const store = useStreamStore();
      store.seedApprovals([authRequest({ id: "a1" })]);
      store.dropApproval("a1");
      expect(store.approvals.has("a1")).toBe(false);
    });
  });

  describe("status frames", () => {
    it("records the latest task/agent state for a task", () => {
      const store = useStreamStore();
      store.apply(envelope({ kind: "status", payload: { task_state: "working_on", agent_state: "running" } }));
      expect(store.statusByTask.get("t1")).toEqual({ task_state: "working_on", agent_state: "running" });

      store.apply(envelope({ kind: "status", payload: { task_state: "completed", agent_state: "cold" } }));
      expect(store.statusByTask.get("t1")).toEqual({ task_state: "completed", agent_state: "cold" });
    });

    it("ignores a status frame missing either axis", () => {
      const store = useStreamStore();
      store.apply(envelope({ kind: "status", payload: { task_state: "working_on" } }));
      expect(store.statusByTask.has("t1")).toBe(false);
    });
  });

  describe("event routing", () => {
    it("keeps each task's events in its own slice", () => {
      const store = useStreamStore();
      store.apply(envelope({ task_id: "t1", seq: 0, payload: { x: 1 } }));
      store.apply(envelope({ task_id: "t2", seq: 0, payload: { x: 2 } }));
      expect(store.eventsFor("t1")?.get(0)).toEqual({ x: 1 });
      expect(store.eventsFor("t2")?.get(0)).toEqual({ x: 2 });
    });

    it("returns undefined for a task with no events", () => {
      const store = useStreamStore();
      expect(store.eventsFor("missing")).toBeUndefined();
    });
  });
});
