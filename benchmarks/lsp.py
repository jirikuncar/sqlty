"""Latency of ``sqlty server`` as an editor sees it, on a generated project.

The server runs in a subprocess and is spoken to over stdio, as VS Code does. Times are
in seconds:

- ``open``: ``initialize`` to the first diagnostics of an opened module, the wait after
  an editor starts;
- ``edit``: a full-text change to diagnostics of that version, median and p95 of
  ``EDITS`` edits;
- ``hover`` and ``completion``: request to response on a table name in a query,
  summed over ``REQUESTS``, so the total is large enough to budget.
"""

from __future__ import annotations

import json
import os
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

EDITS = 15
REQUESTS = 20
# A server that stalls is killed, so a hung benchmark fails instead of the CI job timing out.
DEADLINE_S = 300


class Server:
    def __init__(self, exe: str, root: Path) -> None:
        self.stderr = tempfile.TemporaryFile()  # noqa: SIM115 (closed in stop)
        self.process = subprocess.Popen(
            [exe, "server"],
            cwd=root,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
        )
        self.watchdog = threading.Timer(DEADLINE_S, self.process.kill)
        self.watchdog.start()
        self.next = 0

    def send(self, message: dict[str, Any]) -> None:
        assert self.process.stdin is not None
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
        self.process.stdin.flush()

    def receive(self) -> dict[str, Any]:
        assert self.process.stdout is not None
        length = 0
        while line := self.process.stdout.readline().strip():
            name, value = line.decode().split(": ", 1)
            if name == "Content-Length":
                length = int(value)
        if not length:
            self.stderr.seek(0)
            log = self.stderr.read().decode()[-2000:]
            raise RuntimeError(f"sqlty server closed its output:\n{log}")
        return json.loads(self.process.stdout.read(length))

    def request(self, method: str, params: object) -> Any:
        self.next += 1
        self.send({"id": self.next, "method": method, "params": params})
        while (message := self.receive()).get("id") != self.next or "method" in message:
            pass
        return message.get("result")

    def diagnostics(self, uri: str, version: int) -> list[Any]:
        while True:
            message = self.receive()
            params = message.get("params") or {}
            if (
                message.get("method") == "textDocument/publishDiagnostics"
                and params.get("uri") == uri
                and params.get("version", version) == version
            ):
                return params["diagnostics"]

    def stop(self) -> float:
        """Shut down; return the peak RSS of the server in MB."""
        self.request("shutdown", None)
        self.send({"method": "exit"})
        _, _, usage = os.wait4(self.process.pid, 0)
        self.watchdog.cancel()
        self.stderr.close()
        return usage.ru_maxrss / (1024 * 1024 if sys.platform == "darwin" else 1024)


def _position(text: str, needle: str) -> dict[str, int]:
    offset = text.index(needle)
    line = text.count("\n", 0, offset)
    return {"line": line, "character": offset - (text.rfind("\n", 0, offset) + 1)}


def measure(exe: str, root: Path, module: Path, table: str) -> dict[str, dict[str, float]]:
    """Open ``module`` in a server for ``root``, edit it and query a ``FROM table`` in it."""
    text = module.read_text()
    uri = module.as_uri()
    doc = {"textDocument": {"uri": uri}}
    server = Server(exe, root)
    start = time.perf_counter()
    server.request("initialize", {"capabilities": {}, "rootUri": root.as_uri()})
    server.send({"method": "initialized", "params": {}})
    opened = {"uri": uri, "languageId": "python", "version": 1, "text": text}
    server.send({"method": "textDocument/didOpen", "params": {"textDocument": opened}})
    server.diagnostics(uri, 1)
    opening = time.perf_counter() - start

    edits = []
    for version in range(2, EDITS + 2):
        text = f"{text}x_{version} = {version}\n"
        change = {
            "textDocument": {"uri": uri, "version": version},
            "contentChanges": [{"text": text}],
        }
        t = time.perf_counter()
        server.send({"method": "textDocument/didChange", "params": change})
        server.diagnostics(uri, version)
        edits.append(time.perf_counter() - t)

    # Inside the table name, as when the cursor rests on it or a name is typed there.
    position = _position(text, f"FROM {table}")
    position["character"] += len("FROM t")
    hover, completion = [], []
    for times, method in ((hover, "textDocument/hover"), (completion, "textDocument/completion")):
        for _ in range(REQUESTS):
            t = time.perf_counter()
            result = server.request(method, {**doc, "position": position})
            times.append(time.perf_counter() - t)
            if not result:
                raise RuntimeError(f"{method} found nothing at {position} of {module}")
    rss = server.stop()
    edits.sort()
    return {
        "lsp_open": {"wall_s": round(opening, 4), "rss_mb": round(rss, 1)},
        "lsp_edit": {"wall_s": round(statistics.median(edits), 4)},
        "lsp_edit_p95": {"wall_s": round(edits[int(len(edits) * 0.95) - 1], 4)},
        "lsp_hover": {"wall_s": round(sum(hover), 4)},
        "lsp_completion": {"wall_s": round(sum(completion), 4)},
    }
