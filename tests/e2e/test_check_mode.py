"""Tests for --check mode functionality."""

import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any


def test_check_mode_with_up_to_date_stubs(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode succeeds when stub files are up to date."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check mode should succeed since stubs are up to date
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 0
    assert "up to date" in result.stdout.lower() or "up-to-date" in result.stdout.lower()


def test_check_mode_with_outdated_stubs(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode fails when stub files are outdated."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Modify the source to add a new query
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Check mode should fail since stubs are outdated
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 1
    assert "difference" in result.stdout.lower()


def test_check_mode_with_missing_stubs(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode fails when stub files are missing."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Don't generate stubs - check mode should fail
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 1
    assert "difference" in result.stdout.lower()


def test_check_mode_with_verbose_flag(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode with --verbose shows diff output."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Modify the source
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Check mode with verbose should show diff
    result = cli([str(tmp_path), "--check", "--verbose"])
    assert result.returncode == 1
    # Should contain diff markers
    output = result.stdout + result.stderr
    assert "---" in output or "+++" in output or "diff" in output.lower()


def test_check_mode_with_multiple_verbose_levels(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode supports multiple verbose levels."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Modify the source
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Test -v flag
    result = cli([str(tmp_path), "--check", "-v"])
    assert result.returncode == 1

    # Test -vv flag
    result = cli([str(tmp_path), "--check", "-vv"])
    assert result.returncode == 1

    # Test -vvv flag (should work same as -vv)
    result = cli([str(tmp_path), "--check", "-vvv"])
    assert result.returncode == 1


def test_check_mode_does_not_modify_files(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode does not modify stub files."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    initial_content = stub_file.read_text()

    # Modify the source
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Check mode should not modify the file
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 1

    # File content should not be modified
    assert stub_file.read_text() == initial_content


def test_check_mode_with_directory_mode(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode works with directory analysis."""
    setup_queries_file(tmp_path)

    (tmp_path / "file1.py").write_text("""from queries import sql

q1 = sql("SELECT id::INTEGER FROM users")
""")

    (tmp_path / "file2.py").write_text("""from queries import sql

q2 = sql("SELECT name::TEXT FROM users")
""")

    # Generate initial stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check mode should succeed
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 0


def test_check_mode_exit_codes(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check mode returns correct exit codes."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Generate stubs
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check with up-to-date stubs should return 0
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 0

    # Modify source
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
""")

    # Check with outdated stubs should return 1
    result = cli([str(tmp_path), "--check"])
    assert result.returncode == 1


def test_check_and_format_command_are_mutually_exclusive(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that --check and --format-command cannot be used together."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Try to use both --check and --format-command together
    result = cli([str(tmp_path), "--check", "--format-command", "cat"])

    # Should fail with error message
    assert result.returncode == 1
    output = result.stdout + result.stderr
    assert "mutually exclusive" in output.lower()
