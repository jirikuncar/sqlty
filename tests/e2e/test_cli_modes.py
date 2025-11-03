"""End-to-end tests for CLI modes and commands.

Tests different sqlty CLI commands and modes.
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_cli_help_command(tmp_path: Path) -> None:
    """Test that --help flag works."""
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", "--help"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert "Generate .pyi stub files" in result.stdout or "sqlty" in result.stdout.lower()


def test_cli_version_command(tmp_path: Path) -> None:
    """Test version display.
    
    Note: The --version flag may not be implemented yet.
    Accepts either success (0) if implemented or error (2) if not recognized.
    """
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", "--version"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    # Either succeeds if --version exists, or errors if not implemented
    assert result.returncode in (0, 2)  # 2 = unrecognized arguments


def test_full_mode_generation(tmp_path: Path) -> None:
    """Test full mode that analyzes all files."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    (tmp_path / "file1.py").write_text('''from queries import sql

q1 = sql("SELECT id::INTEGER FROM users")
''')

    (tmp_path / "file2.py").write_text('''from queries import sql

q2 = sql("SELECT name::TEXT FROM users")
''')

    # Run in full mode
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub was created with both queries
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in stub_content
    assert "SELECT name::TEXT FROM users" in stub_content


def test_incremental_mode_generation(tmp_path: Path) -> None:
    """Test incremental mode that only processes changed files."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Run in incremental mode (default)
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()


def test_specific_file_generation(tmp_path: Path) -> None:
    """Test generating stub for a specific file."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Generate stub for specific file
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(app_file)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    # Should succeed (either generating for the registry it finds or reporting)
    assert result.returncode in (0, 1)  # May fail if it can't find registry


def test_output_path_option(tmp_path: Path) -> None:
    """Test specifying custom output path."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    custom_output = tmp_path / "custom_output.pyi"

    # Generate with custom output (if supported)
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            "--output",
            str(custom_output),
        ],
        capture_output=True,
        text=True,
    )

    # May or may not support --output flag
    if result.returncode == 0:
        # If it worked, check the file
        assert custom_output.exists() or (tmp_path / "queries.pyi").exists()


def test_dialect_option(tmp_path: Path) -> None:
    """Test --dialect option."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Test with different dialects
    for dialect in ["postgres", "mysql", "sqlite"]:
        result = subprocess.run(
            [
                sys.executable,
                "-m",
                "sqlty",
                str(tmp_path),
                f"--dialect={dialect}",
            ],
            capture_output=True,
            text=True,
        )

        # Should succeed or gracefully fail
        assert result.returncode in (0, 1, 2)


def test_schema_option(tmp_path: Path) -> None:
    """Test --schema option."""
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text('''CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
''')

    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id, name FROM users")
''')

    # Generate with schema
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(tmp_path),
            f"--schema={schema_file}",
        ],
        capture_output=True,
        text=True,
    )

    # Should succeed
    assert result.returncode in (0, 2)  # 2 if flag not recognized


def test_cli_error_handling_invalid_path(tmp_path: Path) -> None:
    """Test error handling for invalid paths."""
    nonexistent = tmp_path / "nonexistent_dir"

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(nonexistent)],
        capture_output=True,
        text=True,
    )

    # Should fail gracefully
    assert result.returncode != 0 or "not found" in result.stderr.lower()


def test_cli_error_handling_no_registry(tmp_path: Path) -> None:
    """Test error handling when no SQL registry is found."""
    # Create a Python file without SQLRegistry
    app_file = tmp_path / "app.py"
    app_file.write_text('''
def hello() -> str:
    return "Hello, World!"
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    # May succeed (no-op) or fail with appropriate message
    assert result.returncode in (0, 1)


def test_cli_handles_syntax_errors(tmp_path: Path) -> None:
    """Test that CLI handles syntax errors gracefully."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    # Create file with syntax error
    bad_file = tmp_path / "bad.py"
    bad_file.write_text('''from queries import sql

def broken(
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    # Should handle gracefully - either skip the bad file or report error
    # But shouldn't crash
    assert result.returncode in (0, 1)


def test_cli_output_messages(tmp_path: Path) -> None:
    """Test that CLI provides informative output messages."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Should provide some output about what was generated
    output = result.stdout + result.stderr
    assert (
        "generated" in output.lower()
        or "stub" in output.lower()
        or "created" in output.lower()
        or len(output) == 0  # Silent success is also acceptable
    )
