"""The Claude Code plugin hook `hooks/sqlty_breaking.py`, with a stand-in `sqlty` on `PATH`."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).parent.parent
HOOK = ROOT / "hooks" / "sqlty_breaking.py"
FINDING = "migrations/002.sql:1: [missing-lock-timeout] takes a lock that blocks writes"


@pytest.fixture
def project(tmp_path: Path) -> Path:
    """A committed sqlty project, and a `sqlty` that records its arguments and prints `out`
    (a finding unless the test writes one) with the exit code in `rc`."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    fake = bin_dir / "sqlty"
    fake.write_text(
        f"#!{sys.executable}\nimport pathlib, sys\n"
        f"here = pathlib.Path({str(tmp_path)!r})\n"
        "(here / 'args').write_text(' '.join(sys.argv[1:]))\n"
        f"out = here / 'out'\nprint(out.read_text() if out.exists() else {FINDING!r})\n"
        "rc = here / 'rc'\nsys.exit(int(rc.read_text()) if rc.exists() else 1)\n"
    )
    fake.chmod(0o755)
    repo = tmp_path / "repo"
    (repo / "migrations").mkdir(parents=True)
    (repo / "pyproject.toml").write_text('[tool.sqlty]\nschema = "migrations/*.sql"\n')
    (repo / "migrations" / "001.sql").write_text("CREATE TABLE users (id BIGINT);\n")
    git = ["git", "-c", "user.name=t", "-c", "user.email=t@t"]
    subprocess.run([*git, "init", "-q"], cwd=repo, check=True)
    subprocess.run([*git, "add", "-A"], cwd=repo, check=True)
    subprocess.run([*git, "commit", "-qm", "base"], cwd=repo, check=True)
    return repo


def hook(project: Path, file_path: str) -> str:
    event = {"cwd": str(project), "tool_name": "Edit", "tool_input": {"file_path": file_path}}
    env = {**os.environ, "PATH": f"{project.parent / 'bin'}{os.pathsep}{os.environ['PATH']}"}
    done = subprocess.run(
        [sys.executable, HOOK], input=json.dumps(event), capture_output=True, text=True, env=env
    )
    assert done.returncode == 0, done.stderr
    return done.stdout


def test_an_edited_migration_gets_the_findings(project: Path) -> None:
    (project / "migrations" / "002.sql").write_text("ALTER TABLE users ADD COLUMN x INT;\n")
    output = json.loads(hook(project, "migrations/002.sql"))["hookSpecificOutput"]
    assert output["hookEventName"] == "PostToolUse"
    assert FINDING in output["additionalContext"]
    assert (project.parent / "args").read_text() == "breaking --against HEAD"


def test_a_branch_is_checked_against_where_it_left_origin(project: Path) -> None:
    def git(*args: str) -> str:
        done = subprocess.run(
            ["git", "-c", "user.name=t", "-c", "user.email=t@t", *args],
            cwd=project,
            check=True,
            capture_output=True,
            text=True,
        )
        return done.stdout.strip()

    origin = project.parent / "origin.git"
    subprocess.run(["git", "init", "-q", "--bare", origin], check=True)
    git("remote", "add", "origin", str(origin))
    git("push", "-q", "origin", "HEAD:refs/heads/main")
    git("remote", "set-head", "origin", "main")
    branched = git("rev-parse", "HEAD")
    (project / "migrations" / "002.sql").write_text("ALTER TABLE users ADD COLUMN x INT;\n")
    git("add", "-A")
    git("commit", "-qm", "migration")
    hook(project, "migrations/002.sql")
    assert (project.parent / "args").read_text() == f"breaking --against {branched}"


def test_an_absolute_path_below_the_project_counts(project: Path) -> None:
    assert FINDING in hook(project, str(project / "migrations" / "002.sql"))


@pytest.mark.parametrize("file_path", ["app/models.py", "README.md"])
def test_other_files_run_nothing(project: Path, file_path: str) -> None:
    assert hook(project, file_path) == ""
    assert not (project.parent / "args").exists()


def test_a_project_without_sqlty_runs_nothing(project: Path) -> None:
    (project / "pyproject.toml").write_text("[project]\nname = 'app'\n")
    assert hook(project, "migrations/002.sql") == ""
    assert not (project.parent / "args").exists()


def test_the_manifest_names_the_hook() -> None:
    manifest = json.loads((ROOT / ".claude-plugin" / "plugin.json").read_text())
    hooks = json.loads((ROOT / manifest["hooks"]).read_text())["hooks"]["PostToolUse"]
    assert HOOK.name in hooks[0]["hooks"][0]["command"]


def test_findings_of_the_edited_file_come_first(project: Path) -> None:
    (project.parent / "out").write_text(f"app/q.py:2: [unknown-column] bio\n{FINDING}")
    context = json.loads(hook(project, "migrations/002.sql"))["hookSpecificOutput"]
    assert context["additionalContext"].index(FINDING) < context["additionalContext"].index(
        "app/q.py"
    )


def test_long_output_is_cut(project: Path) -> None:
    (project.parent / "out").write_text(f"{FINDING}\n" * 200)
    context = json.loads(hook(project, "migrations/002.sql"))["hookSpecificOutput"]
    assert "\n...\n" in context["additionalContext"]
    assert len(context["additionalContext"]) < 4200


def test_a_failed_run_says_nothing(project: Path) -> None:
    """Exit code 2 is a usage error, such as a baseline that no longer exists: not a finding."""
    (project.parent / "rc").write_text("2")
    assert hook(project, "migrations/002.sql") == ""
    assert (project.parent / "args").exists()


def test_a_sqlty_toml_marks_the_project(project: Path) -> None:
    (project / "pyproject.toml").unlink()
    (project / "sqlty.toml").write_text('schema = "migrations/*.sql"\n')
    assert FINDING in hook(project, "migrations/002.sql")


def test_the_project_venv_wins_over_path(project: Path) -> None:
    venv = project / ".venv" / "bin"
    venv.mkdir(parents=True)
    (project.parent / "bin" / "sqlty").rename(venv / "sqlty")
    assert FINDING in hook(project, "migrations/002.sql")


def test_a_directory_outside_git_runs_nothing(project: Path) -> None:
    shutil.rmtree(project / ".git")
    assert hook(project, "migrations/002.sql") == ""
    assert not (project.parent / "args").exists()
