"""``schema`` entries: files, directories and globs."""

from pathlib import Path

from sqlty._cli import main


def _tree(tmp_path: Path) -> None:
    (tmp_path / "tables" / "nested").mkdir(parents=True)
    (tmp_path / "tables" / "b.sql").write_text("CREATE TABLE b (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "a.sql").write_text("CREATE TABLE a (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "nested" / "c.sql").write_text("CREATE TABLE c (id INTEGER NOT NULL);")
    (tmp_path / "tables" / "notes.txt").write_text("not sql")
    (tmp_path / "extra.sql").write_text("CREATE TABLE e (id INTEGER NOT NULL);")


def _rewritten(tmp_path: Path, monkeypatch, query: str, *schema: str) -> str:
    source = tmp_path / "q.py"
    source.write_text(f'q = "--sql\\n{query}"\n')
    monkeypatch.chdir(tmp_path)
    main(["rewrite", *(f"--schema={entry}" for entry in schema), "q.py"])
    return source.read_text()


def test_directory_is_recursive(tmp_path: Path, monkeypatch) -> None:
    _tree(tmp_path)
    found = _rewritten(tmp_path, monkeypatch, "SELECT a.id, b.id, c.id FROM a, b, c", "tables")
    assert "SQL[tuple[int, int, int]]" in found


def test_glob_matches_only_its_files(tmp_path: Path, monkeypatch) -> None:
    _tree(tmp_path)
    found = _rewritten(tmp_path, monkeypatch, "SELECT c.id FROM c", "extra.sql", "tables/*.sql")
    assert "SQL[" not in found
    found = _rewritten(tmp_path, monkeypatch, "SELECT c.id FROM c", "**/c.sql")
    assert "SQL[tuple[int]]" in found


def test_missing_schema_files_load_nothing(tmp_path: Path, monkeypatch, caplog) -> None:
    _tree(tmp_path)
    for missing in ("missing.sql", "nothing/*.sql"):
        assert "SQL[" not in _rewritten(tmp_path, monkeypatch, "SELECT a.id FROM a", missing)


def test_cli_schema_repeatable_and_pyproject_list(tmp_path: Path, monkeypatch) -> None:
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
