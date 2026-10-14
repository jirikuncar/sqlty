"""Branches of the CLI, configuration and lint modules that no mdtest reaches."""

from __future__ import annotations

import importlib.metadata
import logging
from pathlib import Path

import pytest

import sqlty
from sqlty._cli import _is_excluded, main
from sqlty._config import ConfigError, load_config, validate_tool_table
from sqlty._lint import RULES, RuleConfig, Severity
from sqlty.core import json_shapes


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


def test_rules_must_be_a_table() -> None:
    with pytest.raises(ConfigError, match="rules must be a table"):
        validate_tool_table({"rules": ["select-star"]}, "pyproject.toml")


def test_unreadable_pyproject_is_skipped_with_a_warning(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    (tmp_path / "pyproject.toml").write_text("[tool.sqlty\n")
    with caplog.at_level(logging.WARNING):
        config, root = load_config(tmp_path)
    assert (config, root) == ({}, None)
    assert "Failed to parse" in caplog.text


def test_incomplete_function_entries_are_skipped(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nfunctions = ["junk", {import-path = "db.run"},'
        ' {import-path = "db.ok", query-param = "q"}]\n'
    )
    with caplog.at_level(logging.WARNING):
        config, _ = load_config(tmp_path)
    assert config["functions"] == [{"import_path": "db.ok", "query_param": "q"}]
    assert "missing required fields" in caplog.text


def test_rule_config_ignores_unknown_rules_and_bad_severities() -> None:
    config = RuleConfig({"no-such-rule": "error", "select-star": "loud", "stale-type": "ignore"})
    assert config.is_enabled("select-star") == (
        RULES["select-star"].default_severity is not Severity.IGNORE
    )
    assert not config.is_enabled("stale-type")


def test_exclude_glob_outside_the_config_directory_never_matches(tmp_path: Path) -> None:
    project = tmp_path / "project"
    project.mkdir()
    outside = tmp_path / "other" / "gen.py"
    assert not _is_excluded(outside, project.resolve(), ("gen.py", "other"))


def test_json_shapes_of_a_broken_query_is_empty() -> None:
    assert json_shapes("SELEC (") == []


def test_version_falls_back_when_not_installed(monkeypatch: pytest.MonkeyPatch) -> None:
    def missing(name: str) -> str:
        raise importlib.metadata.PackageNotFoundError(name)

    monkeypatch.setattr(importlib.metadata, "version", missing)
    monkeypatch.delitem(vars(sqlty), "__version__", raising=False)
    try:
        assert sqlty.__version__ == "0.0.0"
    finally:
        vars(sqlty).pop("__version__", None)
