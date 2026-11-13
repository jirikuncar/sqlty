"""Write ``sw.js`` into a ``make docs`` build: a service worker that keeps the site offline.

    python3 scripts/offline.py site

It caches every page and asset when it installs, and the playground (its engine and Monaco,
most of the bytes) once a page asks for it: the playground does on its first visit, and so
does every page of the installed app. wllama, which only the playground's ask loads, is cached
when it is fetched. A new build changes ``sw.js``, which replaces the cache and keeps what
was cached of these.
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

PLAYGROUND = ("assets/playground/", "assets/vendor/monaco/")
ON_USE = ("assets/vendor/wllama/", "assets/vendor/wllama-compat/")
SKIP = ("sw.js", ".integrity", ".md", ".txt")

WORKER = """\
const CACHE = "sqlty-%(version)s";
const SITE = %(site)s;
const PLAYGROUND = %(playground)s;
const ON_USE = %(on_use)s;

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      // A build that replaces one with the playground or wllama cached keeps them cached.
      const had = await caches.match("assets/playground/sqlty_wasm.wasm");
      const used = new Set();
      for (const key of await caches.keys()) {
        if (!key.startsWith("sqlty-") || key === CACHE) continue;
        for (const request of await (await caches.open(key)).keys()) {
          if (ON_USE.some((prefix) => request.url.startsWith(new URL(prefix, registration.scope).href))) used.add(request.url);
        }
      }
      const cache = await caches.open(CACHE);
      await cache.addAll([...SITE, ...(had ? PLAYGROUND : []), ...used]);
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key.startsWith("sqlty-") && key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("message", (event) => {
  if (event.data === "playground") event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(PLAYGROUND)));
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET" || new URL(request.url).origin !== location.origin) return;
  event.respondWith(
    caches.open(CACHE).then(async (cache) => {
      const hit = await cache.match(request, { ignoreSearch: request.mode === "navigate" });
      if (hit) return hit;
      try {
        const response = await fetch(request);
        if (response.ok) cache.put(request, response.clone());
        return response;
      } catch (error) {
        if (request.mode === "navigate") return (await cache.match("404.html")) ?? Response.error();
        throw error;
      }
    }),
  );
});
"""


def url(path: str) -> str:
    if path == "index.html":
        return "./"
    return path.removesuffix("index.html")


def main() -> None:
    site = Path(sys.argv[1])
    files = sorted(
        path.relative_to(site).as_posix()
        for path in site.rglob("*")
        if path.is_file() and not path.name.endswith(SKIP)
    )
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.encode() + b"\0" + (site / path).read_bytes())
    playground = [url(path) for path in files if path.startswith(PLAYGROUND)]
    rest = [url(path) for path in files if not path.startswith(PLAYGROUND + ON_USE)]
    (site / "sw.js").write_text(
        WORKER
        % {
            "version": digest.hexdigest()[:12],
            "site": json.dumps(rest),
            "playground": json.dumps(playground),
            "on_use": json.dumps(ON_USE),
        }
    )
    print(f"{site / 'sw.js'}: {len(rest)} files on install, {len(playground)} for the playground")


if __name__ == "__main__":
    main()
