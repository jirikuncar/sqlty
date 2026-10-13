"""End-to-end tests for per-file ``pyproject.toml`` resolution in the CLI.

``tests/test_cli.py`` covers multi-path rewrite, ``--dialect`` and ``untyped``;
these cover nested projects, ``strip``, ``--schema``, rules per project and
the monorepo case (a ``pyproject.toml`` without ``[tool.sqlty]`` is skipped)."""

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

Cli = Callable[..., subprocess.CompletedProcess[str]]

QUERY = 'q = "--sql\\nSELECT id, name FROM users"\n'


def _project(root: Path, name: str, config: str | None) -> Path:
    project = root / name
    project.mkdir()
    (project / "queries.py").write_text(QUERY)
    if config is not None:
        (project / "schema.sql").write_text("CREATE TABLE users (id INTEGER, name TEXT);\n")
        (project / "pyproject.toml").write_text(f"[tool.sqlty]\n{config}")
    return project


@pytest.fixture
def projects(tmp_path: Path) -> dict[str, Path]:
    """Three sibling projects: no config, a schema, and a schema plus import style."""
    return {
        "plain": _project(tmp_path, "plain", None),
        "schema": _project(tmp_path, "schema", 'schema = "schema.sql"\n'),
        "styled": _project(
            tmp_path,
            "styled",
            'schema = "schema.sql"\nimport-style = "from sqlty import SQL as Q"\n',
        ),
    }


EXPECTED = {
    "plain": QUERY,
    "schema": 'from sqlty import SQL\nq = SQL[tuple[int, str]]("--sql\\nSELECT id, name FROM users")\n',
    "styled": 'from sqlty import SQL as Q\nq = Q[tuple[int, str]]("--sql\\nSELECT id, name FROM users")\n',
}


def test_rewrite_parent_directory_uses_nested_configs(cli: Cli, projects: dict[str, Path]):
    result = cli(["rewrite", projects["plain"].parent])
    assert result.returncode == 0, result.stderr
    assert {name: (p / "queries.py").read_text() for name, p in projects.items()} == EXPECTED


def test_check_and_strip_use_each_paths_config(cli: Cli, projects: dict[str, Path]):
    paths = list(projects.values())
    assert cli(["rewrite", *paths]).returncode == 0

    check = cli(["check", paths[0].parent])
    assert check.returncode == 0, check.stdout + check.stderr
    assert check.stdout == ""

    assert cli(["strip", *paths]).returncode == 0
    assert {(p / "queries.py").read_text() for p in paths} == {QUERY}


def test_cli_schema_overrides_every_config(cli: Cli, projects: dict[str, Path], tmp_path: Path):
    override = tmp_path / "override.sql"
    override.write_text("CREATE TABLE users (id TEXT, name INTEGER);\n")
    result = cli(["rewrite", "--schema", override, projects["plain"], projects["schema"]])
    assert result.returncode == 0, result.stderr
    for name in ("plain", "schema"):
        assert "SQL[tuple[str, int]]" in (projects[name] / "queries.py").read_text()


def test_rewrite_single_file_uses_its_config(cli: Cli, projects: dict[str, Path]):
    result = cli(["rewrite", "../schema/queries.py"], {"cwd": projects["styled"]})
    assert result.returncode == 0, result.stderr
    assert (projects["schema"] / "queries.py").read_text() == EXPECTED["schema"]
    assert (projects["styled"] / "queries.py").read_text() == QUERY


def test_monorepo_package_without_sqlty_table_uses_root_config(cli: Cli, tmp_path: Path):
    root = _project(tmp_path, "mono", 'schema = "schema.sql"\n')
    package = root / "pkg"
    package.mkdir()
    (package / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
    (package / "a.py").write_text(QUERY)
    result = cli(["rewrite", root])
    assert result.returncode == 0, result.stderr
    assert (package / "a.py").read_text() == EXPECTED["schema"]
    assert (root / "queries.py").read_text() == EXPECTED["schema"]


def test_check_rules_per_project(cli: Cli, tmp_path: Path):
    _project(
        tmp_path, "strict", 'schema = "schema.sql"\n[tool.sqlty.rules]\nstale-rewrite = "error"\n'
    )
    _project(
        tmp_path, "lenient", 'schema = "schema.sql"\n[tool.sqlty.rules]\nstale-rewrite = "ignore"\n'
    )
    result = cli(["check", tmp_path])
    assert result.returncode == 1
    assert "strict" in result.stdout and "stale-rewrite" in result.stdout
    assert "lenient" not in result.stdout
