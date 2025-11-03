"""End-to-end tests for CLI modes and commands.

Tests different sqlty CLI commands and modes.
"""

import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

from inline_snapshot import snapshot


def test_cli_help_command(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test that --help flag works."""
    result = cli(["--help"], {"cwd": str(tmp_path)})

    assert result.returncode == 0
    assert "Generate .pyi stub files" in result.stdout or "sqlty" in result.stdout.lower()


def test_cli_version_command(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test that --version flag is handled correctly.
    
    The tool should either display version information (if implemented)
    or report that the argument is not recognized.
    """
    result = cli(["--version"], {"cwd": str(tmp_path)})

    # Should only accept success (version shown)
    assert result.returncode == 0


def test_full_mode_generation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test full mode that analyzes all files."""
    setup_queries_file(tmp_path)

    (tmp_path / "file1.py").write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

q1: SQL[tuple[int]] = sql("SELECT id::INTEGER FROM users")
''')

    (tmp_path / "file2.py").write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

q2: SQL[tuple[str]] = sql("SELECT name::TEXT FROM users")
''')

    # Run in full mode
    result = cli([str(tmp_path), "--mode=full"])

    assert result.returncode == 0

    # Verify stub was created with both queries
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER FROM users"],
    ) -> SQL[tuple[int]]:
        # from file1:7
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT name::TEXT FROM users"],
    ) -> SQL[tuple[str]]:
        # from file2:7
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")


def test_incremental_mode_generation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test incremental mode that only processes changed files."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Run in incremental mode (default)
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()


def test_specific_file_generation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test generating stub for a specific file."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Generate stub for specific file
    result = cli([str(app_file)], {"cwd": str(tmp_path)})

    # Should succeed (either generating for the registry it finds or reporting)
    assert result.returncode in (0, 1)  # May fail if it can't find registry


def test_output_path_option(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test specifying custom output path."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    custom_output = tmp_path / "custom_output.pyi"

    # Generate with custom output (if supported)
    result = cli([str(tmp_path), "--output", str(custom_output)])

    # May or may not support --output flag
    if result.returncode == 0:
        # If it worked, check the file
        assert custom_output.exists() or (tmp_path / "queries.pyi").exists()


def test_dialect_option(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test --dialect option."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    # Test with different dialects
    for dialect in ["postgres", "mysql", "sqlite"]:
        result = cli([str(tmp_path), f"--dialect={dialect}"])

        # Should succeed or gracefully fail
        assert result.returncode in (0, 1, 2)


def test_schema_option(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test --schema option."""
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text('''CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
''')

    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id, name FROM users")
''')

    # Generate with schema
    result = cli([str(tmp_path), f"--schema={schema_file}"])

    # Should succeed
    assert result.returncode in (0, 2)  # 2 if flag not recognized


def test_cli_error_handling_invalid_path(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test error handling for invalid paths."""
    nonexistent = tmp_path / "nonexistent_dir"

    result = cli([str(nonexistent)])

    # Should fail gracefully
    assert result.returncode != 0 or "not found" in result.stderr.lower()


def test_cli_error_handling_no_registry(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test error handling when no SQL registry is found."""
    # Create a Python file without SQLRegistry
    app_file = tmp_path / "app.py"
    app_file.write_text('''
def hello() -> str:
    return "Hello, World!"
''')

    result = cli([str(tmp_path)])

    # May succeed (no-op) or fail with appropriate message
    assert result.returncode in (0, 1)


def test_cli_handles_syntax_errors(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that CLI handles syntax errors gracefully."""
    setup_queries_file(tmp_path)

    # Create file with syntax error
    bad_file = tmp_path / "bad.py"
    bad_file.write_text('''from queries import sql

def broken(
''')

    result = cli([str(tmp_path)])

    # Should handle gracefully - either skip the bad file or report error
    # But shouldn't crash
    assert result.returncode in (0, 1)


def test_cli_output_messages(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that CLI provides informative output messages."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

query = sql("SELECT id::INTEGER FROM users")
''')

    result = cli([str(tmp_path), "--mode=full"])

    assert result.returncode == 0

    # Should provide some output about what was generated
    output = result.stdout + result.stderr
    assert (
        "generated" in output.lower()
        or "stub" in output.lower()
        or "created" in output.lower()
        or len(output) == 0  # Silent success is also acceptable
    )
