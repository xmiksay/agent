import { useSessionStore } from "../stores/session";
import type { InboundMessage, StreamEnvelope } from "../types/api";

export interface TaskStreamHandlers {
  /** A frame arrived from the server. */
  onEnvelope: (env: StreamEnvelope) => void;
  /** The socket (re)connected — a good moment to refetch history and close gaps. */
  onOpen?: () => void;
  /** The socket closed (session end or a transient drop). */
  onClose?: () => void;
  /** Gate auto-reconnect: return false once the task is terminal. */
  shouldReconnect?: () => boolean;
}

export interface TaskStreamController {
  send: (msg: InboundMessage) => void;
  close: () => void;
}

/**
 * Open the live task stream. Reconnects with capped backoff while
 * `shouldReconnect()` holds; `close()` stops it for good. The bearer token (if
 * any) rides as a query param since browsers can't set headers on a WebSocket.
 */
export function openTaskStream(
  taskId: string,
  handlers: TaskStreamHandlers,
): TaskStreamController {
  const session = useSessionStore();
  let ws: WebSocket | null = null;
  let closed = false;
  let retry = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const url = () => {
    const base = location.origin.replace(/^http/, "ws");
    const token = session.token;
    const q = token ? `?token=${encodeURIComponent(token)}` : "";
    return `${base}/ws/tasks/${taskId}${q}`;
  };

  const connect = () => {
    if (closed) return;
    ws = new WebSocket(url());
    ws.onopen = () => {
      retry = 0;
      handlers.onOpen?.();
    };
    ws.onmessage = (ev) => {
      try {
        handlers.onEnvelope(JSON.parse(ev.data as string) as StreamEnvelope);
      } catch {
        /* ignore unparseable frames */
      }
    };
    ws.onclose = () => {
      handlers.onClose?.();
      if (closed || handlers.shouldReconnect?.() === false) return;
      // Backoff: 0.5s, 1s, 2s, 4s … capped at 10s.
      const delay = Math.min(10_000, 500 * 2 ** retry++);
      timer = setTimeout(connect, delay);
    };
    ws.onerror = () => ws?.close();
  };

  connect();

  return {
    send(msg) {
      if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
    },
    close() {
      closed = true;
      if (timer !== null) clearTimeout(timer);
      ws?.close();
    },
  };
}
