import json
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

import pytest

from sqlty._config import ConfigError
from sqlty._lsp_bridge import Workspace
from sqlty.core import format_source

if TYPE_CHECKING:
    from sqlty._breaking import Direction

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


def test_buffers_are_rewritten_and_stripped_with_project_settings(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nschema = "schema.sql"\nimport-style = "import sqlty"\n'
    )
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER NOT NULL);\n")
    workspace = Workspace(str(tmp_path))
    q = str(tmp_path / "q.py")
    plain = 'q = "--sql\\nSELECT id FROM users"\n'

    rewritten = workspace.rewrite(q, plain)

    assert rewritten.splitlines()[-1] == 'q = sqlty.SQL[tuple[int]]("--sql\\nSELECT id FROM users")'
    assert workspace.strip(q, rewritten) == plain
    assert workspace.format(q, plain) == format_source(plain, import_style="import sqlty")
    assert workspace.breaking() == {}


OLD_SCHEMA = "CREATE TABLE users (id INTEGER, bio TEXT);\n"
NEW_SCHEMA = "CREATE TABLE users (id INTEGER, handle TEXT);\n"
OLD_CODE = 'bios = "--sql\\nSELECT bio FROM users"\n'
NEW_CODE = 'handles = "--sql\\nSELECT handle FROM users"\n'


def git(repo: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    """A repository with `bio`, whose working tree replaces it with `handle`."""
    (tmp_path / "app").mkdir()
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nschema = "schema.sql"\nrules = { stale-rewrite = "ignore" }\n'
    )
    (tmp_path / "schema.sql").write_text(OLD_SCHEMA)
    (tmp_path / "app" / "old.py").write_text(OLD_CODE)
    git(tmp_path, "init", "-q")
    git(tmp_path, "add", "-A")
    git(
        tmp_path, "-c", "user.email=t@example.com", "-c", "user.name=t", "commit", "-q", "-m", "old"
    )
    (tmp_path / "schema.sql").write_text(NEW_SCHEMA)
    return tmp_path


def test_buffers_are_checked_forward_against_the_baseline(repo: Path) -> None:
    workspace = Workspace(str(repo / "app"), against="HEAD")
    new = str(repo / "app" / "new.py")
    found = [d for d in workspace.check(new, NEW_CODE) if d.get("source")]
    assert [(d["rule"], d["message"], d["fix"]) for d in found] == [
        (
            "unknown-column",
            "column `handle` does not exist in `users` (with the schema of HEAD)",
            [],
        )
    ]
    assert found[0].get("source") == "sqlty breaking (forward)"


def test_the_baseline_is_checked_backward_once(repo: Path) -> None:
    workspace = Workspace(str(repo / "app"), against="HEAD")
    old = repo / "app" / "old.py"
    [(path, [finding])] = workspace.breaking().items()
    assert path == str(old)
    assert finding["message"] == "column `bio` does not exist in `users` (in the code of HEAD)"
    assert finding.get("source") == "sqlty breaking (backward)"
    assert OLD_CODE[finding["start"] : finding["end"]] == "bio"

    def backward(text: str) -> list[tuple[str, int]]:
        found = workspace.check(str(old), text)
        return [
            (d["message"], d["start"])
            for d in found
            if d.get("source") == "sqlty breaking (backward)"
        ]

    assert backward(OLD_CODE) == [(finding["message"], finding["start"])]
    edited = "# edited\n" + OLD_CODE
    assert backward(edited) == [
        ("column `bio` does not exist in `users` (in the code of HEAD, line 1)", 0)
    ]


def test_a_baseline_file_deleted_from_the_workspace(repo: Path) -> None:
    """Its backward findings stand at the top of the file, as for an edited one."""
    (repo / "app" / "old.py").unlink()
    workspace = Workspace(str(repo / "app"), against="HEAD")

    [(path, [finding])] = workspace.breaking().items()

    assert path == str(repo / "app" / "old.py")
    assert (finding["start"], finding["end"]) == (0, 0)
    assert finding["message"].endswith("(in the code of HEAD, line 1)")


def test_a_schema_file_baseline_outside_a_repository(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(NEW_SCHEMA)
    (tmp_path / "old.sql").write_text(OLD_SCHEMA)
    workspace = Workspace(str(tmp_path), against="old.sql")

    assert workspace.breaking() == {}
    found = workspace.check(str(tmp_path / "q.py"), NEW_CODE)
    assert [d["message"] for d in found if d.get("source")] == [
        "column `handle` does not exist in `users` (with the schema of old.sql)"
    ]


@pytest.mark.parametrize("direction", ["forward", "backward"])
def test_one_direction(repo: Path, direction: Direction) -> None:
    workspace = Workspace(str(repo), against="HEAD", direction=direction)
    found = workspace.check(str(repo / "app" / "new.py"), NEW_CODE)
    forward = [d for d in found if d.get("source") == "sqlty breaking (forward)"]
    assert (len(forward), len(workspace.breaking())) == (
        (1, 0) if direction == "forward" else (0, 1)
    )


def test_an_unchanged_schema_is_not_checked_again(
    repo: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import sqlty.core

    (repo / "schema.sql").write_text(OLD_SCHEMA)
    workspace = Workspace(str(repo / "app"), against="HEAD")
    monkeypatch.setattr(sqlty.core, "breaking_source", None)
    found = workspace.check(str(repo / "app" / "new.py"), NEW_CODE)
    assert [d["rule"] for d in found] == ["unknown-column"]
    assert workspace.breaking() == {}


def test_a_baseline_outside_git_must_be_a_file(tmp_path: Path) -> None:
    with pytest.raises(ConfigError, match="main does not exist, and .* is in no git repository"):
        Workspace(str(tmp_path), against="main")
    with pytest.raises(ConfigError, match="unknown revision 'nope'"):
        git(tmp_path, "init", "-q")
        Workspace(str(tmp_path), against="nope")


class Server:
    """``sqlty server`` in a subprocess, spoken to as an editor does."""

    def __init__(self, root: Path, *args: str, stderr: int | None = None) -> None:
        self.process = subprocess.Popen(
            [sys.executable, *(args or ("-m", "sqlty", "server"))],
            cwd=root,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr,
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


# A baseline sends a workspace through Python; one equal to the schema reports nothing more.
BASELINES = pytest.mark.parametrize("baseline", [None, "baseline.sql"])


def initialize(root: Path, baseline: str | None) -> dict[str, Any]:
    params: dict[str, Any] = {"capabilities": {}, "rootUri": root.as_uri()}
    if baseline is not None:
        schema = root / "schema.sql"
        (root / baseline).write_text(schema.read_text() if schema.exists() else "")
        params["initializationOptions"] = {"breaking": {"against": str(root / baseline)}}
    return params


@BASELINES
def test_server_checks_and_fixes_a_buffer(tmp_path: Path, baseline: str | None) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER, score REAL);\n")
    uri = (tmp_path / "q.py").as_uri()
    text = 'from sqlty import SQL\n\nq = SQL[tuple[int]]("--sql\\nSELECT score FROM users")\n'
    server = Server(tmp_path)
    try:
        server.request("initialize", initialize(tmp_path, baseline))
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


@BASELINES
def test_server_formats_the_query_strings_of_a_buffer(tmp_path: Path, baseline: str | None) -> None:
    (tmp_path / "sqlty.toml").write_text('dialect = "sqlite"\n')
    uri = (tmp_path / "q.py").as_uri()
    text = 'q = """--sql\nselect id,name from users where id=?"""\n'
    server = Server(tmp_path)
    try:
        server.request("initialize", initialize(tmp_path, baseline))
        server.send({"method": "initialized", "params": {}})
        document = {"uri": uri, "languageId": "python", "version": 1, "text": text}
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
        server.notification("textDocument/publishDiagnostics")

        start = {"line": 0, "character": 0}
        here = {"textDocument": {"uri": uri}, "range": {"start": start, "end": start}}
        only = {**here, "context": {"diagnostics": [], "only": ["source.format"]}}
        [action] = server.request("textDocument/codeAction", only)
        assert action["kind"] == "source.format.sqlty"
        [edit] = action["edit"]["documentChanges"][0]["edits"]
        assert edit == {
            "range": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 35}},
            "newText": "\n    id,\n    name\nfrom users\nwhere id = ",
        }
    finally:
        assert server.stop() == 0


@BASELINES
def test_server_checks_the_query_files_of_a_project(tmp_path: Path, baseline: str | None) -> None:
    (tmp_path / "sqlty.toml").write_text('schema = "schema.sql"\nqueries = "sql/"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER, email TEXT);\n")
    (tmp_path / "sql").mkdir()
    queries, other = (tmp_path / "sql" / "users.sql").as_uri(), (tmp_path / "schema.sql").as_uri()
    text = "-- name: by-email^\nSELECT id, emial FROM users WHERE email = :email;\n"
    server = Server(tmp_path)
    try:
        server.request("initialize", initialize(tmp_path, baseline))
        server.send({"method": "initialized", "params": {}})
        for uri in (queries, other):
            document = {"uri": uri, "languageId": "sql", "version": 1, "text": text}
            server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
            published = server.notification("textDocument/publishDiagnostics")
            assert published["uri"] == uri
            codes = [(d["code"], d["range"]["start"]) for d in published["diagnostics"]]
            expected = [("unknown-column", {"line": 1, "character": 11})] if uri == queries else []
            assert codes == expected

        start = {"line": 1, "character": 0}
        here = {"textDocument": {"uri": queries}, "range": {"start": start, "end": start}}
        fix_all = {**here, "context": {"diagnostics": [], "only": ["source.fixAll"]}}
        assert server.request("textDocument/codeAction", fix_all) in (None, [])
        tokens = server.request(
            "textDocument/semanticTokens/full", {"textDocument": {"uri": queries}}
        )
        assert tokens is None
    finally:
        assert server.stop() == 0


@BASELINES
def test_server_completes_and_highlights_helper_and_marked_queries(
    tmp_path: Path, baseline: str | None
) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER NOT NULL, name TEXT);\n")
    (tmp_path / "db.py").write_text(HELPER)
    uri = (tmp_path / "q.py").as_uri()
    text = 'from db import run\n\nrun("SELECT u. FROM users u")\nq = "--sql\\nSELECT * FROM us"\n'
    server = Server(tmp_path)
    try:
        init = server.request("initialize", initialize(tmp_path, baseline))
        assert init["capabilities"]["completionProvider"]["triggerCharacters"] == ["."]
        server.send({"method": "initialized", "params": {}})
        document = {"uri": uri, "languageId": "python", "version": 1, "text": text}
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
        server.notification("textDocument/publishDiagnostics")

        def complete(line: int, character: int) -> Any:
            position = {"line": line, "character": character}
            return server.request(
                "textDocument/completion", {"textDocument": {"uri": uri}, "position": position}
            )

        found = complete(2, 14)
        assert [item["label"] for item in found["items"]] == ["id", "name"]
        assert found["items"][0]["textEdit"]["range"]["start"] == {"line": 2, "character": 14}
        resolved = server.request("completionItem/resolve", found["items"][0])
        assert resolved["detail"] == "INT NOT NULL"
        assert [item["label"] for item in complete(3, 28)["items"]] == ["users"]
        assert complete(0, 3) is None

        position = {"line": 2, "character": 25}
        hovered = server.request(
            "textDocument/hover", {"textDocument": {"uri": uri}, "position": position}
        )
        assert hovered["contents"]["value"].startswith("Table `public.users`")

        legend = init["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
        tokens = server.request("textDocument/semanticTokens/full", {"textDocument": {"uri": uri}})
        data = tokens["data"]
        kinds = [legend[kind] for kind in data[3::5]]
        # `u. FROM` is `u.FROM` to the tokenizer until the column is typed.
        assert kinds == ["keyword", "comment", "keyword", "operator", "keyword"]
        assert data[:5] == [2, 5, 6, 0, 0], "SELECT of the helper string"
    finally:
        assert server.stop() == 0


@BASELINES
def test_server_rewrites_and_strips_a_buffer(tmp_path: Path, baseline: str | None) -> None:
    (tmp_path / "sqlty.toml").write_text('schema = "missing.sql"\n')
    uri = (tmp_path / "q.py").as_uri()
    plain = 'q = "--sql\\nSELECT 1"\n'
    server = Server(tmp_path, stderr=subprocess.PIPE)
    try:
        server.request("initialize", initialize(tmp_path, baseline))
        server.send({"method": "initialized", "params": {}})

        def action(kind: str, text: str) -> str:
            document = {"uri": uri, "languageId": "python", "version": 1, "text": text}
            server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
            server.notification("textDocument/publishDiagnostics")
            start = {"line": 0, "character": 0}
            here = {"textDocument": {"uri": uri}, "range": {"start": start, "end": start}}
            only = {**here, "context": {"diagnostics": [], "only": [kind]}}
            [found] = server.request("textDocument/codeAction", only)
            [edit] = found["edit"]["documentChanges"][0]["edits"]
            return str(edit["newText"])

        rewritten = action("source.rewrite.sqlty", plain)
        assert rewritten == 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1")'
        assert action("source.strip.sqlty", rewritten + "\n") == 'q = "--sql\\nSELECT 1"'
    finally:
        assert server.stop() == 0
    assert server.process.stderr is not None
    assert b"Schema file not found" in server.process.stderr.read()


def test_completion_context_of_a_buffer(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "sqlite"\n')
    (tmp_path / "db.py").write_text(HELPER)
    workspace = Workspace(str(tmp_path))
    text = 'from db import run\n\nrun("SELECT 1")\nrun(query)\n'
    workspace.check(str(tmp_path / "q.py"), text)
    context = workspace.completion(str(tmp_path / "q.py"))
    assert context == {
        "schema": None,
        "dialect": "sqlite",
        "import_style": None,
        "helper_starts": [text.index('"SELECT')],
    }


@BASELINES
def test_server_shows_why_a_project_does_not_load(tmp_path: Path, baseline: str | None) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nuntyped = "nope"\n')
    (tmp_path / "q.py").write_text("x = 1\n")
    server = Server(tmp_path)
    try:
        server.request("initialize", initialize(tmp_path, baseline))
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


FAILING = """\
import sqlty._lsp_bridge as bridge
from sqlty._core import serve

class Failing(bridge.Workspace):
    def __init__(self, root, against=None, direction="both"):
        super().__init__(root)

    def check(self, path, text):
        raise RuntimeError("check failed")

    def breaking(self):
        raise RuntimeError("baseline failed")

bridge.Workspace = Failing
serve()
"""


def test_server_logs_what_the_workspace_raises(tmp_path: Path) -> None:
    server = Server(tmp_path, "-c", FAILING)
    try:
        options = {"breaking": {"against": "HEAD"}}
        server.request(
            "initialize",
            {"capabilities": {}, "rootUri": tmp_path.as_uri(), "initializationOptions": options},
        )
        server.send({"method": "initialized", "params": {}})
        uri = (tmp_path / "q.py").as_uri()
        text = 'q = "--sql\\nSELECT 1"\n'
        document = {"uri": uri, "languageId": "python", "version": 1, "text": text}
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})
        logged = ""
        while "check failed" not in logged or "baseline failed" not in logged:
            logged += server.notification("window/logMessage")["message"]
        assert "Traceback" in logged
        start = {"line": 0, "character": 0}
        new_texts = {
            kind: [
                edit["newText"]
                for action in server.request(
                    "textDocument/codeAction",
                    {
                        "textDocument": {"uri": uri},
                        "range": {"start": start, "end": start},
                        "context": {"diagnostics": [], "only": [kind]},
                    },
                )
                for edit in action["edit"]["documentChanges"][0]["edits"]
            ]
            for kind in ("source.rewrite.sqlty", "source.strip.sqlty")
        }
        assert new_texts == {
            "source.rewrite.sqlty": [
                'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1")'
            ],
            "source.strip.sqlty": [],
        }
    finally:
        assert server.stop() == 0


def test_server_reports_breaking_changes_against_a_baseline(repo: Path) -> None:
    server = Server(repo)
    try:
        options = {"breaking": {"against": "HEAD", "direction": "both"}}
        server.request(
            "initialize",
            {"capabilities": {}, "rootUri": repo.as_uri(), "initializationOptions": options},
        )
        server.send({"method": "initialized", "params": {}})
        new = repo / "app" / "new.py"
        document = {"uri": new.as_uri(), "languageId": "python", "version": 1, "text": NEW_CODE}
        server.send({"method": "textDocument/didOpen", "params": {"textDocument": document}})

        old = server.notification("textDocument/publishDiagnostics")
        assert old["uri"] == (repo / "app" / "old.py").as_uri()
        assert [(d["source"], d["message"]) for d in old["diagnostics"]] == [
            (
                "sqlty breaking (backward)",
                "column `bio` does not exist in `users` (in the code of HEAD)",
            )
        ]
        published = server.notification("textDocument/publishDiagnostics")
        assert published["uri"] == new.as_uri()
        assert [(d["source"], d["message"]) for d in published["diagnostics"]] == [
            (
                "sqlty breaking (forward)",
                "column `handle` does not exist in `users` (with the schema of HEAD)",
            )
        ]
    finally:
        assert server.stop() == 0
    worktrees = subprocess.run(
        ["git", "worktree", "list"], cwd=repo, capture_output=True, text=True
    ).stdout
    assert len(worktrees.splitlines()) == 1, "the baseline's worktree is removed on exit"
