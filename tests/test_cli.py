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


def test_check_fix_writes_fixes_and_reports_the_rest(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text("CREATE TABLE users (id INTEGER, name TEXT);")
    (tmp_path / "q.py").write_text(
        "from sqlty import SQL\n"
        'stale = SQL[tuple[str]]("--sql\\nSELECT id FROM users")\n'
        'star = SQL[tuple[int, str]]("--sql\\nSELECT * FROM users")\n'
        'new = "--sql\\nSELECT name FROM users"\n'
        'bad = "--sql SELECT id FROM users"\n'
        'dynamic = f"--sql\\nSELECT {column} FROM users"\n'
    )
    monkeypatch.chdir(tmp_path)

    main(["check", "--fix", "."])

    assert (tmp_path / "q.py").read_text() == (
        "from sqlty import SQL\n"
        'stale = SQL[tuple[int]]("--sql\\nSELECT id FROM users")\n'
        'star = SQL[tuple[int, str]]("--sql\\nSELECT id::INT, name::TEXT FROM users")\n'
        'new = SQL[tuple[str]]("--sql\\nSELECT name FROM users")\n'
        'bad = SQL[tuple[int]]("--sql\\nSELECT id FROM users")\n'
        'dynamic = f"--sql\\nSELECT {column} FROM users"\n'
    )
    out = capsys.readouterr().out
    assert out.startswith("q.py:6: [fstring-skipped]")
    assert "Fixed 1 file(s)" in out


def test_invalid_untyped_config_is_an_error(tmp_path) -> None:
    (tmp_path / "pyproject.toml").write_text("[tool.sqlty]\nuntyped = true\n")
    args = build_parser().parse_args(["check", str(tmp_path)])

    with pytest.raises(SystemExit, match="untyped must be 'skip' or 'sentinel'"):
        SettingsResolver(args).for_file(tmp_path / "q.py")


def test_invalid_import_style_config_is_an_error(tmp_path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nimport-style = "from x import SQL"\n')
    args = build_parser().parse_args(["check", str(tmp_path)])

    with pytest.raises(SystemExit, match="Unsupported import-style: 'from x import SQL'"):
        SettingsResolver(args).for_file(tmp_path / "q.py")


F_SKIPPED = "q.py:2: [fstring-skipped] f-string with --sql prefix skipped (f-strings cannot be statically rewritten)"
T_SKIPPED = "q.py:3: [tstring-skipped] t-string with --sql prefix skipped (t-strings cannot be statically rewritten)"


@pytest.mark.parametrize(
    ("config", "flags", "expected"),
    [
        ("", ["--diff"], [F_SKIPPED, T_SKIPPED]),
        ("", ["--check"], [F_SKIPPED, T_SKIPPED]),
        ("", ["-q"], []),
        ('[tool.sqlty.rules]\nfstring-skipped = "ignore"\n', [], [T_SKIPPED]),
    ],
)
def test_rewrite_reports_skipped_strings(tmp_path, monkeypatch, capsys, config, flags, expected):
    (tmp_path / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}")
    (tmp_path / "q.py").write_text('x = 1\na = f"--sql\\nSELECT {x}"\nb = t"--sql\\nSELECT {x}"\n')
    monkeypatch.chdir(tmp_path)

    main(["rewrite", *flags, "q.py"])

    out, err = capsys.readouterr()
    assert err.splitlines() == expected
    assert "skipped" not in out


def test_unparseable_schema_table_is_reported_once(tmp_path, monkeypatch, caplog) -> None:
    """A table the parser rejects even without its constraints is named in a warning."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(
        "CREATE TABLE ok (id INTEGER CHECK (id > 0));\nCREATE TABLE public.bad (id INTEGER DEFAULT %s);"
    )
    for name in ("a.py", "b.py"):
        (tmp_path / name).write_text('q = "--sql\\nSELECT id FROM ok"\n')
    monkeypatch.chdir(tmp_path)

    main(["rewrite", "a.py", "b.py"])

    assert [r.getMessage() for r in caplog.records if "could not be parsed" in r.getMessage()] == [
        "Schema table public.bad could not be parsed; its columns are unknown"
    ]
