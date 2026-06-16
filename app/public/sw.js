// Minimal offline shell. Cache the app shell on install; serve cache-first for
// navigations so the wallet opens offline. Live LP/oracle/bridge calls always
// go to the network (and are verified on-device regardless of source). M4 will
// flesh this out with precise asset hashing.
const CACHE = "satusd-shell-v2";
const SHELL = [
  "/", "/index.html", "/manifest.webmanifest",
  "/icon.svg", "/icon-192.png", "/icon-512.png",
];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))),
    ).then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET") return;
  // Never cache live data planes — always hit the network.
  if (["/lp", "/oracle", "/bridge"].some((p) => url.pathname.startsWith(p))) return;
  e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request)));
});
