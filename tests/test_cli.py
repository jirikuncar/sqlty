"""CLI behavior that needs files on disk: config discovery and flag precedence."""

from __future__ import annotations

import logging
import os
from pathlib import Path

import pytest

from sqlty._cli import SettingsResolver, _is_excluded, build_parser, main
from sqlty._config import ConfigError
from sqlty._lint import RULES


def test_cli_rewrite_untyped(tmp_path, monkeypatch):
    """``untyped`` comes from [tool.sqlty]; ``--untyped`` overrides it."""
    source = 'q = "--sql\\nSELECT x, y FROM pairs"\n'
    sentinel = 'from sqlty import SQL\nfrom sqlty import UNTYPED\nq = SQL[UNTYPED]("--sql\\nSELECT x, y FROM pairs")\n'
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
        (tmp_path / name / "q.py").write_text(
            source.replace("id FROM", "id::INTEGER FROM", 1) if name == "b" else source
        )
    (tmp_path / "a" / "schema.sql").write_text("CREATE TABLE users (id INTEGER NOT NULL);")
    monkeypatch.chdir(tmp_path)

    main(["rewrite", "b", "a"])

    assert (tmp_path / "a" / "q.py").read_text() == (
        'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT id FROM users")\n'
    )
    assert (tmp_path / "b" / "q.py").read_text() == (
        'import sqlty\nq = sqlty.SQL[tuple[int]]("--sql\\nSELECT id::INTEGER FROM users")\n'
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
    (tmp_path / "schema.sql").write_text(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);"
    )
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

    with pytest.raises(ConfigError, match="untyped must be 'skip' or 'sentinel'"):
        SettingsResolver(args).for_file(tmp_path / "q.py")


def test_invalid_import_style_config_is_an_error(tmp_path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nimport-style = "from x import SQL"\n')
    args = build_parser().parse_args(["check", str(tmp_path)])

    with pytest.raises(ConfigError, match="Unsupported import-style: 'from x import SQL'"):
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


def test_check_statistics_counts_queries(tmp_path, monkeypatch, capsys) -> None:
    """``--statistics`` ends with one line counting queries by how far sqlty types them."""
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nschema = "schema.sql"\n\n[tool.sqlty.rules]\nunparsed-query = "warn"\n'
    )
    (tmp_path / "schema.sql").write_text(
        "CREATE TABLE users (id INTEGER NOT NULL, name TEXT NOT NULL);"
    )
    (tmp_path / "q.py").write_text(
        "from sqlty import SQL\n"
        'typed = SQL[tuple[int]]("--sql\\nSELECT id FROM users")\n'
        'partial = "--sql\\nSELECT id, nope FROM users"\n'
        'broken = "--sql\\nSELECT xmlelement(name foo)"\n'
        'dynamic = f"--sql\\nSELECT {column} FROM users"\n'
        'ddl = "--sql\\nCREATE TABLE t (a INT)"\n'
    )
    monkeypatch.chdir(tmp_path)

    main(["check", "--statistics", "."])
    assert capsys.readouterr().out.splitlines()[-1] == (
        "Queries: 1 typed, 1 partially typed, 1 unparsed, 1 skipped (fstring-skipped: 1)"
    )

    main(["check", "--statistics", "-q", "."])
    assert capsys.readouterr().out == ""


def test_unparseable_schema_view_is_reported(tmp_path, monkeypatch, caplog) -> None:
    """A view the parser rejects is named in a warning, like a table."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    (tmp_path / "schema.sql").write_text(
        "CREATE TABLE ok (id INTEGER);\nCREATE VIEW public.bad (a) AS SELECT id FROM (;"
    )
    (tmp_path / "a.py").write_text('q = "--sql\\nSELECT id FROM ok"\n')
    monkeypatch.chdir(tmp_path)

    main(["rewrite", "a.py"])

    assert [r.getMessage() for r in caplog.records if "could not be parsed" in r.getMessage()] == [
        "Schema view public.bad could not be parsed; its columns are unknown"
    ]


def test_type_modules_config_reaches_the_resolver(tmp_path) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ntype-modules = ["app", "lib.types"]\n')
    args = build_parser().parse_args(["check", str(tmp_path)])

    settings = SettingsResolver(args).for_file(tmp_path / "q.py")

    assert settings.type_modules == ("app", "lib.types")


def test_configured_functions_apply_to_their_own_project(tmp_path, monkeypatch, capsys) -> None:
    """``[[tool.sqlty.functions]]`` of one project do not check another project's files."""
    call = 'from lib import run\n\nrun(q="--sql\\nSELECT 1::INTEGER AS n", row=tuple[str])\n'
    function = '[[tool.sqlty.functions]]\nimport-path = "lib.run"\nquery-param = "q"\n'
    for name, config in (("a", function + 'return-type-param = "row"\n'), ("b", "")):
        (tmp_path / name).mkdir()
        (tmp_path / name / "pyproject.toml").write_text(
            f'[tool.sqlty.rules]\nstale-rewrite = "ignore"\n\n[tool.sqlty]\n{config}'
        )
        (tmp_path / name / "q.py").write_text(call)
    monkeypatch.chdir(tmp_path)

    with pytest.raises(SystemExit):
        main(["check", "a/q.py", "b/q.py"])

    out = capsys.readouterr().out
    assert "a/q.py" in out
    assert "b/q.py" not in out


def _run(argv: list[str], capsys) -> tuple[int, str, str]:
    """Run ``main`` and return (exit code, stdout, stderr)."""
    code = 0
    try:
        main(argv)
    except SystemExit as exc:
        code = exc.code if isinstance(exc.code, int) else 1
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def _project(tmp_path, config: str) -> None:
    (tmp_path / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}")
    (tmp_path / "q.py").write_text("x = 1\n")


def test_unknown_rule_is_a_clean_error(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty.rules]\nselect-starr = "error"\n')
    monkeypatch.chdir(tmp_path)

    code, out, err = _run(["check", "."], capsys)

    assert code == 2
    assert "Traceback" not in err
    assert "pyproject.toml: unknown rule 'select-starr'" in err
    assert "did you mean 'select-star'?" in err


def test_invalid_severity_is_a_clean_error(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty.rules]\nselect-star = "fatal"\n')
    monkeypatch.chdir(tmp_path)

    code, _, err = _run(["check", "."], capsys)

    assert code == 2
    assert "invalid severity 'fatal' for rule 'select-star'" in err


def test_unknown_key_is_an_error(tmp_path, monkeypatch, capsys) -> None:
    _project(tmp_path, "bogus = 1\n")
    monkeypatch.chdir(tmp_path)

    code, _, err = _run(["check", "."], capsys)

    assert code == 2
    assert "unknown key 'bogus' in [tool.sqlty]" in err


def test_snake_case_key_suggests_the_kebab_case_one(tmp_path, monkeypatch, capsys) -> None:
    _project(tmp_path, 'import_style = "import sqlty"\n')
    monkeypatch.chdir(tmp_path)

    code, _, err = _run(["rewrite", "."], capsys)

    assert code == 2
    assert "did you mean 'import-style'?" in err


def test_missing_path_is_an_error(tmp_path, monkeypatch, capsys) -> None:
    monkeypatch.chdir(tmp_path)

    for command in ("check", "rewrite", "strip"):
        code, _, err = _run([command, "nonexist.py"], capsys)
        assert code == 2
        assert "path does not exist: nonexist.py" in err


QUERY = 'q = "--sql\\nSELECT 1::INTEGER AS n"\n'


def test_walk_skips_default_excludes(tmp_path, monkeypatch, capsys) -> None:
    for name in (".venv/lib", "node_modules/pkg", ".git/hooks", "src"):
        (tmp_path / name).mkdir(parents=True)
        (tmp_path / name / "m.py").write_text(QUERY)
    monkeypatch.chdir(tmp_path)

    code, out, _ = _run(["rewrite", "--check", "."], capsys)

    assert code == 1
    assert "Would rewrite " + str(Path("src/m.py")) in out
    assert ".venv" not in out and "node_modules" not in out and ".git" not in out


def test_explicit_files_and_excluded_roots_are_still_checked(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / ".venv").mkdir()
    (tmp_path / ".venv" / "m.py").write_text(QUERY)
    monkeypatch.chdir(tmp_path)

    assert _run(["rewrite", "--check", ".venv/m.py"], capsys)[0] == 1
    assert _run(["rewrite", "--check", ".venv"], capsys)[0] == 1


def test_exclude_and_extend_exclude(tmp_path, monkeypatch, capsys) -> None:
    for name in ("migrations", "gen/api", "keep", ".venv"):
        (tmp_path / name).mkdir(parents=True)
        (tmp_path / name / "m.py").write_text(QUERY)
    (tmp_path / "keep" / "skip_me.py").write_text(QUERY)
    monkeypatch.chdir(tmp_path)

    def checked(config: str) -> set[str]:
        (tmp_path / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}")
        _, out, _ = _run(["rewrite", "--check", "."], capsys)
        return {line.split()[-1] for line in out.splitlines() if line.startswith("Would")}

    sep = os.sep
    assert checked('extend-exclude = ["migrations", "gen/*", "skip_me.py"]\n') == {f"keep{sep}m.py"}
    # `exclude` replaces the defaults, so .venv is walked again.
    assert checked('exclude = ["migrations", "gen", "keep"]\n') == {f".venv{sep}m.py"}


def test_exclude_is_relative_to_the_config_file(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / "pkg" / "gen").mkdir(parents=True)
    (tmp_path / "gen").mkdir()
    for path in ("pkg/gen/m.py", "gen/m.py"):
        (tmp_path / path).write_text(QUERY)
    (tmp_path / "pkg" / "pyproject.toml").write_text('[tool.sqlty]\nextend-exclude = ["gen"]\n')
    monkeypatch.chdir(tmp_path)

    _, out, _ = _run(["rewrite", "--check", "."], capsys)

    assert f"gen{os.sep}m.py" in out
    assert f"pkg{os.sep}gen" not in out


@pytest.mark.parametrize("command", ["rewrite", "strip"])
def test_exit_codes(command, tmp_path, monkeypatch, capsys) -> None:
    wrapped = 'from sqlty import SQL\nq = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
    (tmp_path / "q.py").write_text(QUERY if command == "rewrite" else wrapped)
    monkeypatch.chdir(tmp_path)

    assert _run([command, "--check", "q.py"], capsys)[0] == 1
    assert _run([command, "--diff", "q.py"], capsys)[0] == 1
    assert (tmp_path / "q.py").read_text() == (QUERY if command == "rewrite" else wrapped)
    assert _run([command, "q.py"], capsys)[0] == 0
    assert _run([command, "--check", "q.py"], capsys)[0] == 0
    assert _run([command, "--diff", "q.py"], capsys)[0] == 0


def test_check_exit_codes(tmp_path, monkeypatch, capsys) -> None:
    (tmp_path / "ok.py").write_text("x = 1\n")
    (tmp_path / "bad.py").write_text(
        'from sqlty import SQL\nq = SQL[tuple[int, int]]("--sql\\nSELECT 1::INTEGER AS n")\n'
    )
    monkeypatch.chdir(tmp_path)

    assert _run(["check", "ok.py"], capsys)[0] == 0
    # stale-type is a warning by default: reported, exit 0.
    assert _run(["check", "bad.py"], capsys)[0] == 0
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty.rules]\nstale-type = "error"\n')
    assert _run(["check", "bad.py"], capsys)[0] == 1


def test_internal_error_exits_2(tmp_path, monkeypatch, capsys) -> None:
    import sqlty.core

    def boom(*args, **kwargs):
        raise RuntimeError("boom")

    (tmp_path / "q.py").write_text("x = 1\n")
    monkeypatch.setattr(sqlty.core, "strip_source", boom)

    code, _, err = _run(["strip", str(tmp_path / "q.py")], capsys)

    assert code == 2
    assert "internal error" in err


def _run(argv: list[str], capsys: pytest.CaptureFixture[str]) -> tuple[int, str, str]:
    code = 0
    try:
        main(argv)
    except SystemExit as exc:
        code = exc.code if isinstance(exc.code, int) else 1
    captured = capsys.readouterr()
    return code, captured.out, captured.err


def test_version_flag_and_command(capsys: pytest.CaptureFixture[str]) -> None:
    for argv in (["--version"], ["version"]):
        code, out, _ = _run(argv, capsys)
        assert code == 0
        assert out.startswith("sqlty ")


def test_no_command_prints_help(capsys: pytest.CaptureFixture[str]) -> None:
    code, out, _ = _run([], capsys)
    assert code == 0
    assert "usage: sqlty" in out


def test_rule_command(capsys: pytest.CaptureFixture[str], caplog: pytest.LogCaptureFixture) -> None:
    code, out, _ = _run(["rule"], capsys)
    assert code == 0
    assert all(name in out for name in RULES)

    code, out, _ = _run(["rule", "select-star"], capsys)
    assert code == 0
    assert out.startswith("# select-star")

    code, _, _ = _run(["rule", "no-such-rule"], capsys)
    assert code == 2
    assert "Unknown rule 'no-such-rule'" in caplog.text


@pytest.mark.parametrize("command", ["rewrite", "strip", "check"])
def test_empty_directory_warns(
    command: str,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    caplog: pytest.LogCaptureFixture,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.chdir(tmp_path)
    with caplog.at_level(logging.WARNING):
        code, _, _ = _run([command, "."], capsys)
    assert code == 0
    assert "No Python files found" in caplog.text
    caplog.clear()
    with caplog.at_level(logging.WARNING):
        _run([command, ".", "--quiet"], capsys)
    assert "No Python files found" not in caplog.text


def test_verbose_names_each_changed_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.chdir(tmp_path)
    query = '"--sql\\nSELECT 1::INTEGER AS n"'
    (tmp_path / "q.py").write_text(f"q = {query}\n")
    _, out, _ = _run(["rewrite", "q.py", "--verbose"], capsys)
    assert "q.py" in out
    assert "SQL[" in (tmp_path / "q.py").read_text()
    _, out, _ = _run(["strip", "q.py", "--verbose"], capsys)
    assert "q.py" in out
    assert "SQL[" not in (tmp_path / "q.py").read_text()

    (tmp_path / "f.py").write_text(
        'from sqlty import SQL\nq = SQL[tuple[str]]("--sql\\nSELECT 1::INTEGER")\n'
    )
    _, out, _ = _run(["check", "f.py", "--fix", "--verbose"], capsys)
    assert "Fixed f.py" in out


def test_exclude_glob_outside_the_config_directory_never_matches(tmp_path: Path) -> None:
    project = tmp_path / "project"
    project.mkdir()
    outside = tmp_path / "other" / "gen.py"
    assert not _is_excluded(outside, project.resolve(), ("gen.py", "other"))
