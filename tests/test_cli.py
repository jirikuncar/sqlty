"""CLI behavior that needs files on disk: config discovery and flag precedence."""

from __future__ import annotations

import pytest

from sqlty._cli import SettingsResolver, build_parser, main


def test_cli_rewrite_untyped(tmp_path, monkeypatch):
    """``untyped`` comes from [tool.sqlty]; ``--untyped`` overrides it."""
    source = 'q = "--sql\\nVALUES (1, 2)"\n'
    sentinel = 'from sqlty import SQL\nfrom sqlty import UNTYPED\nq = SQL[UNTYPED]("--sql\\nVALUES (1, 2)")\n'
    target = tmp_path / "q.py"
    monkeypatch.chdir(tmp_path)

    def rewrite(config: str, *flags: str) -> str:
        (tmp_path / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}")
        target.write_text(source)
        main(["rewrite", *flags, str(tmp_path)])
        return target.read_text()

    assert rewrite("") == source
    assert rewrite('untyped = "sentinel"\n') == sentinel
    assert rewrite("", "--untyped", "sentinel") == sentinel
    assert rewrite('untyped = "sentinel"\n', "--untyped", "skip") == source
    with pytest.raises(SystemExit):
        main(["rewrite", "--untyped", "bogus", str(tmp_path)])


def test_each_file_uses_its_nearest_config(tmp_path, monkeypatch, capsys) -> None:
    """One invocation over two projects applies each project's [tool.sqlty]."""
    source = 'q = "--sql\\nSELECT id FROM users"\n'
    for name, config in (("a", 'schema = "schema.sql"'), ("b", 'import-style = "import sqlty"')):
        (tmp_path / name).mkdir()
        (tmp_path / name / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}\n")
        (tmp_path / name / "q.py").write_text(source)
    (tmp_path / "a" / "schema.sql").write_text("CREATE TABLE users (id INTEGER);")
    monkeypatch.chdir(tmp_path)

    main(["rewrite", "b", "a"])

    assert (tmp_path / "a" / "q.py").read_text() == (
        'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT id FROM users")\n'
    )
    assert (tmp_path / "b" / "q.py").read_text() == (
        'import sqlty\nq = sqlty.SQL[tuple[str]]("--sql\\nSELECT id FROM users")\n'
    )
    main(["check", "a", "b"])
    assert capsys.readouterr().out == ""


@pytest.mark.parametrize(("flag", "expected"), [(None, "mysql"), ("postgres", "postgres")])
def test_dialect_flag_overrides_config(tmp_path, flag, expected) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    args = build_parser().parse_args(
        ["check", *(["--dialect", flag] if flag else []), str(tmp_path)]
    )

    assert SettingsResolver(args).for_file(tmp_path / "q.py").dialect == expected
