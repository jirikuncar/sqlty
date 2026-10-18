"""End-to-end tests for ``sqlty breaking``: two git revisions, checked across.

The diagnostics themselves are specified by ``docs/guide/breaking-changes.md``; these
cover the git side: revisions, worktree cleanup, directions, paths and exit codes."""

import json
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

Cli = Callable[..., subprocess.CompletedProcess[str]]

OLD_SCHEMA = "CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, bio TEXT);\n"
NEW_SCHEMA = "CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT NOT NULL, handle TEXT);\n"
OLD_CODE = 'bios = "--sql\\nSELECT bio FROM users"\n'
NEW_CODE = 'handles = "--sql\\nSELECT handle FROM users"\n'
BACKWARD = "app/q.py:1: [unknown-column] column `bio` does not exist in `users` (backward: "
FORWARD = "app/q.py:1: [unknown-column] column `handle` does not exist in `users` (forward: "


def git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=repo, check=True, capture_output=True, text=True
    ).stdout


def commit(repo: Path, schema: str, code: str, message: str) -> None:
    (repo / "schema.sql").write_text(schema)
    (repo / "app" / "q.py").write_text(code)
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    """A repository whose second commit drops `bio` and adds `handle`, code included."""
    repo = tmp_path / "repo"
    (repo / "app").mkdir(parents=True)
    git(repo, "init", "-q")
    git(repo, "config", "user.email", "test@example.com")
    git(repo, "config", "user.name", "test")
    (repo / "pyproject.toml").write_text('[tool.sqlty]\nschema = "schema.sql"\n')
    commit(repo, OLD_SCHEMA, OLD_CODE, "old")
    commit(repo, NEW_SCHEMA, NEW_CODE, "new")
    return repo


def lines(result: subprocess.CompletedProcess[str]) -> list[str]:
    return result.stdout.splitlines()


def test_both_directions_between_two_refs(cli: Cli, repo: Path) -> None:
    result = cli(["breaking", "--against", "HEAD~1", "--head", "HEAD"], {"cwd": repo})

    assert result.returncode == 1, result.stderr
    assert lines(result)[0].startswith(BACKWARD)
    assert lines(result)[0].endswith("the queries of HEAD~1 against the schema of HEAD)")
    assert lines(result)[1].startswith(FORWARD)
    assert lines(result)[2] == "Found 2 breaking change(s)"
    assert git(repo, "worktree", "list").count("\n") == 1
    assert git(repo, "status", "--porcelain") == ""


def test_head_defaults_to_the_working_tree(cli: Cli, repo: Path) -> None:
    (repo / "app" / "q.py").write_text('emails = "--sql\\nSELECT email FROM users"\n')

    result = cli(["breaking", "--against", "HEAD~1", "--direction", "forward"], {"cwd": repo})

    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout == ""


def test_direction_selects_one_check(cli: Cli, repo: Path) -> None:
    result = cli(
        ["breaking", "--against", "HEAD~1", "--head", "HEAD", "--direction", "backward"],
        {"cwd": repo},
    )

    assert result.returncode == 1
    assert [line[: len(BACKWARD)] for line in lines(result)[:-1]] == [BACKWARD]


def test_paths_limit_the_files_in_both_revisions(cli: Cli, repo: Path) -> None:
    (repo / "other").mkdir()
    (repo / "other" / "q.py").write_text(OLD_CODE)

    result = cli(["breaking", "--against", "HEAD", "app"], {"cwd": repo})

    assert result.returncode == 0, result.stdout + result.stderr


def test_output_formats(cli: Cli, repo: Path) -> None:
    args = ["breaking", "--against", "HEAD~1", "--head", "HEAD", "--direction", "forward"]

    result = cli([*args, "--output-format", "json"], {"cwd": repo})
    assert result.returncode == 1
    assert json.loads(result.stdout) == [
        {
            "file": "app/q.py",
            "rule": "unknown-column",
            "message": "column `handle` does not exist in `users`",
            "severity": "error",
            "line": 1,
            "direction": "forward",
        }
    ]

    result = cli([*args, "--output-format", "github"], {"cwd": repo})
    assert result.returncode == 1
    assert result.stdout == (
        "::error file=app/q.py,line=1,title=sqlty unknown-column::column `handle` does not exist "
        "in `users` (forward: the queries of HEAD against the schema of HEAD~1)\n"
    )


def test_quiet_prints_nothing(cli: Cli, repo: Path) -> None:
    result = cli(["breaking", "--against", "HEAD~1", "--head", "HEAD", "-q"], {"cwd": repo})

    assert (result.returncode, result.stdout) == (1, "")


@pytest.mark.parametrize(
    ("ref", "message"),
    [
        ("nope", "sqlty: error: unknown revision 'nope'"),
        ("HEAD~5", "sqlty: error: unknown revision 'HEAD~5'"),
    ],
)
def test_unknown_revision_is_an_error(cli: Cli, repo: Path, ref: str, message: str) -> None:
    result = cli(["breaking", "--against", ref], {"cwd": repo})

    assert result.returncode == 2
    assert result.stderr.strip() == message
    assert git(repo, "worktree", "list").count("\n") == 1


def test_outside_a_repository_is_an_error(cli: Cli, tmp_path: Path) -> None:
    result = cli(["breaking", "--against", "HEAD"], {"cwd": tmp_path})

    assert result.returncode == 2
    assert result.stderr.startswith("sqlty: error: fatal: not a git repository")
