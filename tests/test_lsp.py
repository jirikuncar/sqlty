from pathlib import Path

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
