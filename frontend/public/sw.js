// Service worker for the installable (PWA) build.
//
// Strategy is deliberately network-first: an installed home-screen app must
// always reflect the live site, so every request goes to the network first and
// the cache is only an offline fallback. This means a new deploy is picked up on
// the next online load with no stale-shell problem — the cache never wins while
// the operator is connected. skipWaiting + clients.claim let an updated worker
// take over immediately rather than waiting for every tab to close.

const CACHE = "agent-shell-v1";
// Same-origin prefixes that must never be intercepted/cached — they are live
// data or upgrade to a socket. The SW only sees fetch(), so /ws is moot here,
// but listing it keeps intent obvious.
const BYPASS = ["/api", "/ws", "/webhook", "/internal", "/health"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.add("/index.html")),
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (BYPASS.some((p) => url.pathname.startsWith(p))) return;

  // SPA navigations resolve to index.html; serve the cached shell when offline.
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request)
        .then((res) => {
          caches.open(CACHE).then((c) => c.put("/index.html", res.clone()));
          return res;
        })
        .catch(() => caches.match("/index.html")),
    );
    return;
  }

  // Static assets: network-first, falling back to whatever was last cached.
  event.respondWith(
    fetch(request)
      .then((res) => {
        if (res.ok) {
          const copy = res.clone();
          caches.open(CACHE).then((c) => c.put(request, copy));
        }
        return res;
      })
      .catch(() => caches.match(request)),
  );
});
