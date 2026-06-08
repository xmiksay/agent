// Browser notifications for pending "cmd ask" messages (operator approvals).
//
// Every pending command-approval request collapses into ONE notification (fixed
// tag) whose title reflects the live pending count, so a burst of asks never
// spams the operator. Clicking it focuses the app on the auth queue (handled by
// the service worker's `notificationclick`, see public/sw.js). We only surface a
// notification while the page is hidden (other tab, minimized, or a backgrounded
// installed PWA) — when it's visible the in-app badge already shows the queue.

import { ref } from "vue";

const TAG = "agent-auth-asks";
const TARGET = "/auth_requests";

export const notificationsSupported =
  typeof window !== "undefined" && "Notification" in window;

export const notificationPermission = ref<NotificationPermission>(
  notificationsSupported ? Notification.permission : "denied",
);

/** Prompt the operator for notification permission (a one-time browser dialog). */
export async function enableNotifications(): Promise<void> {
  if (!notificationsSupported) return;
  try {
    notificationPermission.value = await Notification.requestPermission();
  } catch {
    /* user-agent declined to show the prompt — leave the state unchanged */
  }
}

/** The active SW registration, or null. Uses getRegistration (resolves at once)
 *  rather than `.ready` (which never resolves when no worker is installed, e.g.
 *  in `vite dev`), so the notification path can fall back to `new Notification`. */
async function activeRegistration(): Promise<ServiceWorkerRegistration | null> {
  if (!("serviceWorker" in navigator)) return null;
  try {
    const reg = await navigator.serviceWorker.getRegistration();
    return reg?.active ? reg : null;
  } catch {
    return null;
  }
}

/**
 * Refresh the single grouped approval notification to reflect `count` pending
 * asks. A count of 0 clears it (asks were resolved). `latest` is the most recent
 * command, shown for context. No-op unless permission was granted; only shows
 * while the page is hidden.
 */
export async function syncAuthNotification(count: number, latest?: string): Promise<void> {
  if (!notificationsSupported || notificationPermission.value !== "granted") return;

  const reg = await activeRegistration();

  if (count === 0) {
    if (reg) for (const n of await reg.getNotifications({ tag: TAG })) n.close();
    return;
  }

  if (document.visibilityState === "visible") return;

  const title =
    count === 1 ? "Command approval needed" : `${count} command approvals needed`;
  const body = latest ? `$ ${latest}` : "Open the auth queue to review.";
  const options = {
    tag: TAG,
    body,
    renotify: true,
    requireInteraction: true,
    data: { url: TARGET },
  } as NotificationOptions;

  if (reg) {
    await reg.showNotification(title, options);
  } else {
    // No service worker (dev): the constructor handles its own click inline.
    const n = new Notification(title, options);
    n.onclick = () => {
      window.focus();
      window.location.assign(TARGET);
      n.close();
    };
  }
}
