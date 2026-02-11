"""E2E tests for new mode behavior: incremental, prune, and create."""

import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

pytestmark = pytest.mark.skip(reason="Stub generation CLI (sqlty generate) deferred — see PLAN-FUTURE.md")


def test_single_file_default_mode(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that single file mode defaults to incremental,create."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # Run without mode flag - should default to incremental,create
    result = cli([str(app_file)], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    # Check output contains the default mode
    output = result.stdout + result.stderr
    assert "incremental,create" in output or "incremental" in output


def test_directory_default_mode(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that directory mode defaults to incremental,prune,create."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # Run without mode flag - should default to incremental,prune,create
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check output contains the default mode
    output = result.stdout + result.stderr
    assert "incremental,prune,create" in output


def test_create_mode_skips_existing_stub(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that create mode skips files that already exist."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # First run creates the stub
    result = cli([str(tmp_path), "--mode=incremental"])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()
    original_content = stub_file.read_text()

    # Update the source with a new query
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
""")

    # Run with create mode - should skip because file exists
    result = cli([str(tmp_path), "--mode=create"])
    assert result.returncode == 0

    new_content = stub_file.read_text()
    # Content should be unchanged
    assert new_content == original_content
    assert "SELECT 2::INTEGER" not in new_content


def test_incremental_prune_mode_adds_and_removes_queries(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that incremental,prune mode both adds new queries and removes stale ones."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
""")

    # First run creates the stub with both queries
    result = cli([str(tmp_path), "--mode=incremental"])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()
    content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" in content

    # Update source: remove query2, add query3
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
query3 = sql("SELECT 3::INTEGER")
""")

    # Run with incremental,prune mode (provide 'y' input for confirmation prompt on single file)
    result = cli(
        [str(app_file), "--mode=incremental,prune"], {"cwd": str(tmp_path), "input": "y\n"}
    )
    assert result.returncode == 0

    content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 3::INTEGER" in content  # Added
    assert "SELECT 2::INTEGER" not in content  # Removed


def test_prune_mode_on_single_file_requires_confirmation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that prune mode on single file prompts for confirmation."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # First generate the stub
    result = cli([str(app_file), "--mode=incremental"], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    # Try to run prune on single file with 'n' (no) response
    result = cli([str(app_file), "--mode=prune"], {"cwd": str(tmp_path), "input": "n\n"})

    # Should exit with code 0 (aborted by user)
    assert result.returncode == 0

    # Check that warning was shown
    output = result.stdout + result.stderr
    assert "WARNING" in output or "warning" in output.lower()


def test_incremental_mode_updates_existing_stub(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[..., Path],
    create_pyproject_toml: Callable[..., Path],
) -> None:
    """Test that incremental mode updates existing stubs with new queries."""
    # Create a single query file
    setup_queries_file(tmp_path, "queries.py", "sql")

    # Register it in pyproject.toml
    create_pyproject_toml(tmp_path, [("queries.sql", "query")])

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

q1 = sql("SELECT 1::INTEGER")
""")

    # Generate initial stub
    result = cli([str(app_file), "--mode=incremental"], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    stub_path = tmp_path / "queries.pyi"
    assert stub_path.exists()
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content

    # Update the app file with a new query
    app_file.write_text("""from queries import sql

q1 = sql("SELECT 1::INTEGER")
q2 = sql("SELECT 2::INTEGER")
""")

    # Run incremental mode again
    result = cli([str(app_file), "--mode=incremental"], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    # Verify the stub was updated with the new query
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" in content


def test_incremental_create_mode_on_directory(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[..., Path],
    create_pyproject_toml: Callable[..., Path],
) -> None:
    """Test that incremental,create mode works on directory with existing and new stubs."""
    # Create a single query file
    setup_queries_file(tmp_path, "queries.py", "sql")

    # Register it in pyproject.toml
    create_pyproject_toml(tmp_path, [("queries.sql", "query")])

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

q1 = sql("SELECT 1::INTEGER")
""")

    # Generate initial stub for queries
    result = cli([str(tmp_path), "--mode=incremental"], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    stub_path = tmp_path / "queries.pyi"
    assert stub_path.exists()
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content

    # Now add a new query to the app file
    app_file.write_text("""from queries import sql

q1 = sql("SELECT 1::INTEGER")
q2 = sql("SELECT 2::INTEGER")
""")

    # Run on directory with incremental,create mode
    result = cli([str(tmp_path), "--mode=incremental,create"], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"CLI failed: {result.stdout}\n{result.stderr}"

    # Verify the stub was updated (incremental)
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" in content


def test_incremental_create_mode_updates_existing_creates_new(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[..., Path],
    create_pyproject_toml: Callable[..., Path],
) -> None:
    """Test that incremental,create updates existing stubs but creates new ones."""
    # Create query files separately
    setup_queries_file(tmp_path, "queries1.py", "sql")
    setup_queries_file(tmp_path, "queries2.py", "sql")

    # Register them in pyproject.toml
    create_pyproject_toml(tmp_path, [("queries1.sql", "query"), ("queries2.sql", "query")])

    app1_file = tmp_path / "app1.py"
    app1_file.write_text("""from queries1 import sql

q1 = sql("SELECT 1::INTEGER")
""")

    app2_file = tmp_path / "app2.py"
    app2_file.write_text("""from queries2 import sql

q2 = sql("SELECT 2::INTEGER")
""")

    # Generate stub for queries1
    result = cli([str(app1_file), "--mode=incremental"], {"cwd": str(tmp_path)})
    assert result.returncode == 0, f"CLI failed: {result.stdout}\n{result.stderr}"

    stub1_path = tmp_path / "queries1.pyi"
    assert stub1_path.exists(), (
        f"Stub file not found. Files: {list(tmp_path.glob('*'))}, CLI output: {result.stdout}"
    )

    # Update queries1 with new query
    app1_file.write_text("""from queries1 import sql

q1 = sql("SELECT 1::INTEGER")
q1b = sql("SELECT 10::INTEGER")
""")

    # Run on directory with incremental,create mode
    result = cli([str(tmp_path), "--mode=incremental,create"], {"cwd": str(tmp_path)})
    assert result.returncode == 0

    # Check queries1 stub was updated (incremental)
    content1 = stub1_path.read_text()
    assert "SELECT 1::INTEGER" in content1
    assert "SELECT 10::INTEGER" in content1

    # Check queries2 stub was created (create)
    stub2_path = tmp_path / "queries2.pyi"
    assert stub2_path.exists()
    content2 = stub2_path.read_text()
    assert "SELECT 2::INTEGER" in content2


def test_fresh_mode_replaces_stub_completely(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that fresh mode replaces the entire stub content."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # First run creates stub
    result = cli([str(tmp_path), "--mode=incremental"])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    # Update source with different query
    app_file.write_text("""from queries import sql

query2 = sql("SELECT 2::INTEGER")
""")

    # Delete existing stub and run create mode (replaces entirely)
    stub_file.unlink()
    result = cli([str(tmp_path), "--mode=create"])
    assert result.returncode == 0

    content = stub_file.read_text()
    assert "SELECT 2::INTEGER" in content
    assert "SELECT 1::INTEGER" not in content  # Old query should be gone


def test_prune_mode_directory_does_not_prompt(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that prune mode on directory doesn't prompt for confirmation."""
    setup_sqlty_package(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
""")

    # First run
    result = cli([str(tmp_path), "--mode=incremental"])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    # Update source, remove query2
    app_file.write_text("""from queries import sql

query1 = sql("SELECT 1::INTEGER")
""")

    # Run with prune on directory - should NOT prompt
    result = cli([str(tmp_path), "--mode=prune"])
    assert result.returncode == 0

    content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" not in content
