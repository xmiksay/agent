// Registers the service worker that makes the app installable on a phone's home
// screen. Only in production builds — in `vite dev` a SW would shadow HMR. The
// worker is network-first (see public/sw.js), so registering it changes nothing
// about freshness while online; it only adds installability and an offline shell.

export function registerServiceWorker() {
  if (!import.meta.env.PROD) return;
  if (!("serviceWorker" in navigator)) return;

  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch((err) => {
      console.warn("Service worker registration failed", err);
    });
  });
}
