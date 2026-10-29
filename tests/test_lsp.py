import json
import subprocess
import sys
from pathlib import Path
from typing import Any, cast

from sqlty._lsp_bridge import Workspace

HELPER = """\
from typing import Annotated
from sqlty import SQL

def run(query: Annotated[str, SQL]) -> None: ...
"""
CALL = 'from db import run\n\nrun("SELECT nope FROM users")\n'


def test_buffers_replace_their_files(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER);\n")
    (tmp_path / "db.py").write_text("")
    workspace = Workspace(str(tmp_path))
    db, q = str(tmp_path / "db.py"), str(tmp_path / "q.py")

    assert workspace.check(q, CALL) == []
    workspace.check(db, HELPER)
    assert [d["rule"] for d in workspace.check(q, CALL)] == ["unknown-column"]
    workspace.check(db, HELPER.replace("query: Annotated[str, SQL]", "query: str"))
    assert workspace.check(q, CALL) == []


class Server:
    """``sqlty server`` in a subprocess, spoken to as an editor does."""

    def __init__(self, root: Path) -> None:
        self.process = subprocess.Popen(
            [sys.executable, "-m", "sqlty", "server"],
            cwd=root,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
        )
        self.next = 0
        self.notifications: list[dict[str, Any]] = []

    def send(self, message: dict[str, Any]) -> None:
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        assert self.process.stdin is not None
        self.process.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
        self.process.stdin.flush()

    def receive(self) -> dict[str, Any]:
        assert self.process.stdout is not None
        headers = {}
        while line := self.process.stdout.readline().strip():
            name, value = line.decode().split(": ", 1)
            headers[name] = value
        return cast(
            dict[str, Any], json.loads(self.process.stdout.read(int(headers["Content-Length"])))
        )

    def request(self, method: str, params: object) -> Any:
        self.next += 1
        self.send({"id": self.next, "method": method, "params": params})
        while "id" not in (message := self.receive()):
            self.notifications.append(message)
        assert message["id"] == self.next, message
        return message["result"]

    def notification(self, method: str) -> Any:
        while not (found := [n for n in self.notifications if n["method"] == method]):
            self.notifications.append(self.receive())
        self.notifications.remove(found[0])
        return found[0]["params"]

    def stop(self) -> int:
        self.request("shutdown", None)
        self.send({"method": "exit"})
        return self.process.wait(timeout=10)


def test_server_checks_and_fixes_a_buffer(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER, score REAL);\n")
    uri = (tmp_path / "q.py").as_uri()
    text = 'from sqlty import SQL\n\nq = SQL[tuple[int]]("--sql\\nSELECT score FROM users")\n'
    server = Server(tmp_path)
    try:
        server.request("initialize", {"capabilities": {}, "rootUri": tmp_path.as_uri()})
        server.send({"method": "initialized", "params": {}})
        document = {"uri": uri, "languageId": "python", "version": 1, "text": text}
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
        published = server.notification("textDocument/publishDiagnostics")
        [diagnostic] = published["diagnostics"]
        assert diagnostic["code"] == "narrow-type"
        assert diagnostic["message"] == "SQL[tuple[int]] should be SQL[tuple[float | None]]"
        assert diagnostic["range"] == {
            "start": {"line": 2, "character": 8},
            "end": {"line": 2, "character": 18},
        }

        here = {"textDocument": {"uri": uri}, "range": diagnostic["range"]}
        [quick_fix] = server.request(
            "textDocument/codeAction", {**here, "context": {"diagnostics": []}}
        )
        [edit] = quick_fix["edit"]["documentChanges"][0]["edits"]
        assert edit["newText"] == "tuple[float | None]"

        fix_all = {**here, "context": {"diagnostics": [], "only": ["source.fixAll"]}}
        [action] = server.request("textDocument/codeAction", fix_all)
        [edit] = action["edit"]["documentChanges"][0]["edits"]
        assert edit == {
            "range": {"start": {"line": 2, "character": 14}, "end": {"line": 2, "character": 17}},
            "newText": "float | None",
        }
    finally:
        assert server.stop() == 0


def test_server_shows_why_a_project_does_not_load(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nuntyped = "nope"\n')
    (tmp_path / "q.py").write_text("x = 1\n")
    server = Server(tmp_path)
    try:
        server.request("initialize", {"capabilities": {}, "rootUri": tmp_path.as_uri()})
        server.send({"method": "initialized", "params": {}})
        document = {
            "uri": (tmp_path / "q.py").as_uri(),
            "languageId": "python",
            "version": 1,
            "text": "x = 1\n",
        }
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
        shown = server.notification("window/showMessage")
        assert shown["message"].startswith("sqlty: ")
        assert "untyped must be 'skip' or 'sentinel'" in shown["message"]
    finally:
        assert server.stop() == 0
