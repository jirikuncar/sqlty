"""``schema`` entries: files, directories and globs."""

from pathlib import Path

from sqlty._cli import _load_schema
from sqlty._config import expand_schema


def _tree(tmp_path: Path) -> None:
    (tmp_path / "tables" / "nested").mkdir(parents=True)
    (tmp_path / "tables" / "b.sql").write_text("CREATE TABLE b (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "a.sql").write_text("CREATE TABLE a (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "nested" / "c.sql").write_text("CREATE TABLE c (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "notes.txt").write_text("not sql")
    (tmp_path / "extra.sql").write_text("CREATE TABLE e (id INTEGER NOT NULL);")


def test_directory_is_recursive_and_sorted(tmp_path: Path) -> None:
    _tree(tmp_path)
    names = [p.relative_to(tmp_path).as_posix() for p in expand_schema("tables", tmp_path)]
    assert names == ["tables/a.sql", "tables/b.sql", "tables/nested/c.sql"]


def test_glob_and_order_of_entries(tmp_path: Path) -> None:
    _tree(tmp_path)
    files = expand_schema(["extra.sql", "tables/*.sql", "extra.sql"], tmp_path)
    assert [p.name for p in files] == ["extra.sql", "a.sql", "b.sql"]
    assert [p.name for p in expand_schema("**/c.sql", tmp_path)] == ["c.sql"]


def test_load_schema_joins_files(tmp_path: Path) -> None:
    _tree(tmp_path)
    sql = _load_schema(["tables/a.sql", "extra.sql"], tmp_path)
    assert sql == "CREATE TABLE a (id INTEGER NOT NULL);\nCREATE TABLE e (id INTEGER NOT NULL);"
    assert _load_schema("missing.sql", tmp_path) is None
    assert _load_schema("nothing/*.sql", tmp_path) is None


def test_cli_schema_repeatable_and_pyproject_list(tmp_path: Path, monkeypatch) -> None:
    from sqlty._cli import main

    _tree(tmp_path)
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = ["tables", "extra.sql"]\n')
    src = tmp_path / "q.py"
    src.write_text('q = "--sql\\nSELECT a.id, e.id FROM a, e"\n')
    monkeypatch.chdir(tmp_path)
    main(["rewrite", "q.py"])
    assert "SQL[tuple[int, int]]" in src.read_text()
    src.write_text('q = "--sql\\nSELECT b.id, c.id FROM b, c"\n')
    main(["rewrite", "--schema", "tables/b.sql", "--schema", "tables/nested/*.sql", "q.py"])
    assert "SQL[tuple[int, int]]" in src.read_text()
