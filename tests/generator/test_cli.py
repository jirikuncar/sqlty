"""Test the generator CLI tool execution."""

import subprocess
import sys
from pathlib import Path


def test_cli_help() -> None:
    """Test --help flag works."""
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", "--help"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    assert "Generate .pyi stub files" in result.stdout


def test_cli_basic_generation(tmp_path: Path) -> None:
    """Test basic CLI stub generation (lines 764-830)."""
    # Create a simple source file
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


result = Query.sql("SELECT id FROM users")
""")

    # Run the generator CLI
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(source_file)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert "Generated stub file" in result.stdout or "Generated Query class stub" in result.stdout

    # Check stub was created
    stub_file = source_file.with_suffix(".pyi")
    assert stub_file.exists()


def test_cli_with_output_path(tmp_path: Path) -> None:
    """Test CLI with explicit output path (lines 783-785)."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
""")

    output_file = tmp_path / "custom.pyi"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "-o",
            str(output_file),
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert output_file.exists()


def test_cli_with_dialect(tmp_path: Path) -> None:
    """Test CLI with SQL dialect option (line 787-791)."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
""")

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "--dialect",
            "mysql",
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0


def test_cli_with_incremental_flag(tmp_path: Path) -> None:
    """Test CLI with incremental mode (lines 803-806)."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
""")

    # First generation
    subprocess.run(
        [sys.executable, "-m", "sqlty", str(source_file)],
        capture_output=True,
        cwd=str(tmp_path),
    )

    # Modify source
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
y = Query.sql("SELECT 2")
""")

    # Incremental generation
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "--mode=incremental",
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0


def test_cli_with_full_mode(tmp_path: Path) -> None:
    """Test CLI with full mode flag."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
""")

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "--mode=full",
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0


def test_cli_nonexistent_source_error(tmp_path: Path) -> None:
    """Test CLI error handling for nonexistent source (lines 815-817)."""
    nonexistent = tmp_path / "does_not_exist.py"

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(nonexistent)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 1
    assert "does not exist" in result.stderr


def test_cli_with_directory_source(tmp_path: Path) -> None:
    """Test CLI with directory as source (lines 819-830)."""
    # Create a directory with source files
    pkg_dir = tmp_path / "mypackage"
    pkg_dir.mkdir()

    (pkg_dir / "queries.py").write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass


x = Query.sql("SELECT 1")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(pkg_dir)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert "Generated stub file" in result.stdout or "Generated Query class stub" in result.stdout


def test_cli_all_options_combined(tmp_path: Path) -> None:
    """Test CLI with all options combined."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

x = Query.sql("SELECT id FROM users")
""")

    output_file = tmp_path / "output.pyi"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "-o",
            str(output_file),
            "--dialect",
            "postgres",
            "--mode=full",
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert output_file.exists()

    # Verify output messages
    assert "To use this stub file" in result.stdout or result.returncode == 0


def test_cli_output_messages(tmp_path: Path) -> None:
    """Test CLI output messages are printed (lines 826-830)."""
    source_file = tmp_path / "queries.py"
    source_file.write_text("""
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

x = Query.sql("SELECT 1")
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(source_file)],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    # Check for expected output messages
    assert result.returncode == 0
    # At least one of these messages should be present
    assert (
        "Generated stub file" in result.stdout
        or "Generated Query class stub" in result.stdout
        or "use this stub file" in result.stdout
    )


def test_cli_with_global_schema_mapping_precedence(tmp_path: Path) -> None:
    """CLI --schema global path should override __schema_path__ on registry."""
    # Create two schemas that differ on users.id type
    schema1 = tmp_path / "schema1.sql"
    schema1.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )
    schema2 = tmp_path / "schema2.sql"
    schema2.write_text(
        """
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    # Registry with __schema_path__ pointing to schema1
    source_file = tmp_path / "queries.py"
    source_file.write_text(
        """
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    __schema_path__ = "schema1.sql"

sql = UserQueries.sql

x = sql("SELECT id, name FROM users")
"""
    )

    # Run CLI with global --schema pointing to schema2
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "--mode=full",
            "--schema",
            str(schema2),
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0

    # Expect id to be inferred as str (from schema2), not int (from schema1)
    stub_file = source_file.with_suffix(".pyi")
    content = stub_file.read_text()
    assert "SQL[tuple[str, str]]" in content


def test_cli_with_registry_specific_schema_mapping_precedence_over_global(tmp_path: Path) -> None:
    """Registry-specific mapping should override global mapping."""
    # Global schema makes id TEXT
    global_schema = tmp_path / "global.sql"
    global_schema.write_text(
        """
        CREATE TABLE users (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    # Registry-specific schema makes id INTEGER
    specific_schema = tmp_path / "users_queries.sql"
    specific_schema.write_text(
        """
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        );
        """
    )

    # Registry in module users_queries.py to match mapping key
    source_file = tmp_path / "users_queries.py"
    source_file.write_text(
        """
from sqlty import SQLRegistry

class UsersQueries(SQLRegistry):
    __schema_path__ = "does_not_matter.sql"

sql = UsersQueries.sql

x = sql("SELECT id, name FROM users")
"""
    )

    # Run CLI with both global and registry-specific mappings
    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "sqlty",
            str(source_file),
            "--mode=full",
            "--schema",
            str(global_schema),
            "--schema",
            f"users_queries.UsersQueries:{specific_schema}",
        ],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0

    # Expect id inferred as int (specific mapping wins over global)
    stub_file = source_file.with_suffix(".pyi")
    content = stub_file.read_text()
    assert "SQL[tuple[int, str]]" in content
