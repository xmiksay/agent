import { defineStore } from "pinia";
import { reactive, ref } from "vue";
import { useSessionStore } from "./session";
import type { AuthRequest, StreamEnvelope } from "../types/api";

/** Operator → agent messages on the single global socket (each names its task). */
export type OutboundMessage =
  | { kind: "chat"; task_id: string; text: string }
  | { kind: "redefine"; task_id: string; text: string }
  | { kind: "stop"; task_id: string };

/**
 * One process-wide WebSocket for the whole app. Every task's frames arrive here
 * and are routed by `task_id`, so the browser holds a single connection instead
 * of one per open task. Views read their slice: `eventsFor(taskId)`,
 * `approvals` (across all tasks), and `statusByTask`.
 */
export const useStreamStore = defineStore("stream", () => {
  // taskId → (seq → raw agent event). Inner maps are reactive so computeds that
  // read a single task's events re-run on each new frame.
  const events = reactive(new Map<string, Map<number, unknown>>());
  // authId → AuthRequest, across all tasks (drops out when resolved).
  const approvals = reactive(new Map<string, AuthRequest>());
  // taskId → latest status pushed over the wire.
  const statusByTask = reactive(new Map<string, string>());
  const connected = ref(false);

  let ws: WebSocket | null = null;
  let closed = false;
  let retry = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function url(): string {
    return `${location.origin.replace(/^http/, "ws")}/ws`;
  }

  /** Get (creating if needed) the reactive event map for a task. */
  function ensure(taskId: string): Map<number, unknown> {
    let m = events.get(taskId);
    if (!m) {
      m = reactive(new Map<number, unknown>());
      events.set(taskId, m);
    }
    return m;
  }

  function eventsFor(taskId: string): Map<number, unknown> | undefined {
    return events.get(taskId);
  }

  /** Seed a task's history (array index = seq) from the REST /events endpoint. */
  function seedEvents(taskId: string, items: unknown[]) {
    const m = ensure(taskId);
    items.forEach((e, i) => m.set(i, e));
  }

  /** Seed pending approvals fetched over REST (so they show before any frame). */
  function seedApprovals(list: AuthRequest[]) {
    for (const a of list) approvals.set(a.id, a);
  }

  function dropApproval(id: string) {
    approvals.delete(id);
  }

  function apply(env: StreamEnvelope) {
    if (env.kind === "event") {
      ensure(env.task_id).set(env.seq, env.payload);
    } else if (env.kind === "auth_request") {
      const r = env.payload as AuthRequest;
      if (r.status === "pending") approvals.set(r.id, r);
      else approvals.delete(r.id);
    } else if (env.kind === "status") {
      const s = (env.payload as { status?: string }).status;
      if (s) statusByTask.set(env.task_id, s);
    }
  }

  function connect() {
    if (closed) return;
    ws = new WebSocket(url());
    ws.onopen = () => {
      retry = 0;
      // In-band auth: the token rides as the first frame, not a query param, so
      // it never lands in URLs or proxy logs. The server closes if it mismatches.
      ws?.send(JSON.stringify({ token: useSessionStore().token ?? "" }));
      connected.value = true;
    };
    ws.onmessage = (ev) => {
      try {
        apply(JSON.parse(ev.data as string) as StreamEnvelope);
      } catch {
        /* ignore unparseable frames */
      }
    };
    ws.onclose = () => {
      connected.value = false;
      if (closed) return;
      const delay = Math.min(10_000, 500 * 2 ** retry++);
      timer = setTimeout(connect, delay);
    };
    ws.onerror = () => ws?.close();
  }

  /** Open the socket once (idempotent). No-op until the session is validated —
   *  we don't connect without a usable token (or confirmed no-auth mode). */
  function start() {
    if (useSessionStore().validated !== true) return;
    closed = false;
    if (!ws) connect();
  }

  function stop() {
    closed = true;
    if (timer !== null) clearTimeout(timer);
    ws?.close();
    ws = null;
    connected.value = false;
  }

  function send(msg: OutboundMessage) {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  }

  return {
    events,
    approvals,
    statusByTask,
    connected,
    ensure,
    eventsFor,
    seedEvents,
    seedApprovals,
    dropApproval,
    start,
    stop,
    send,
  };
});
