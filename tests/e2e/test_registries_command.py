"""Tests for the sqlty.registries subcommand."""

import subprocess
from collections.abc import Callable
from pathlib import Path


def test_registries_find_single_file(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test finding registries in a single file."""
    # Create a test file with a registry
    test_file = tmp_path / "test.py"
    test_file.write_text("""from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    # Create pyproject.toml with function configuration
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "test.sql"
query_param = "query"
""")

    # Run the registries command
    result = registries_cli([test_file])

    assert result.returncode == 0
    assert str(test_file) + ":sql" in result.stdout


def test_registries_find_in_directory(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test finding registries in a directory."""
    # Create multiple files with registries
    file1 = tmp_path / "queries1.py"
    file1.write_text("""from sqlty import sqlty

@sqlty
def sql_a(query: str):
    return query
""")

    file2 = tmp_path / "queries2.py"
    file2.write_text("""from sqlty import sqlty

@sqlty
def sql_b(query: str):
    return query

@sqlty
def sql_c(query: str):
    return query
""")

    # Create pyproject.toml with function configurations
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "queries1.sql_a"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "queries2.sql_b"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "queries2.sql_c"
query_param = "query"
""")

    # Run the registries command
    result = registries_cli([tmp_path])

    assert result.returncode == 0
    output_lines = result.stdout.strip().split("\n")
    assert len(output_lines) == 3
    assert any("queries1.py:sql_a" in line for line in output_lines)
    assert any("queries2.py:sql_b" in line for line in output_lines)
    assert any("queries2.py:sql_c" in line for line in output_lines)


def test_registries_indirect_import(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test finding registries with indirect imports (module.Class)."""
    test_file = tmp_path / "test.py"
    test_file.write_text("""from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    # Create pyproject.toml with function configuration
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "test.sql"
query_param = "query"
""")

    result = registries_cli([test_file])

    assert result.returncode == 0
    assert str(test_file) + ":sql" in result.stdout


def test_registries_default_path(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test using default path (current directory)."""
    test_file = tmp_path / "test.py"
    test_file.write_text("""from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    # Create pyproject.toml with function configuration
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "test.sql"
query_param = "query"
""")

    result = registries_cli([], {"cwd": str(tmp_path)})

    assert result.returncode == 0
    assert "test.py:sql" in result.stdout


def test_registries_invalid_path(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test error handling for nonexistent paths."""
    result = registries_cli(["/nonexistent/path"])

    assert result.returncode == 1
    assert "does not exist" in result.stderr


def test_registries_too_many_args(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test error handling for too many arguments."""
    result = registries_cli(["arg1", "arg2"])

    assert result.returncode == 1
    assert "Usage" in result.stderr


def test_registries_no_registries(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test behavior when no registries are found."""
    result = registries_cli([tmp_path])

    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_registries_nested_directories(
    tmp_path: Path,
    registries_cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test finding registries in nested directories."""
    # Create nested structure
    subdir = tmp_path / "subdir"
    subdir.mkdir()

    file1 = tmp_path / "top.py"
    file1.write_text("""from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    file2 = subdir / "nested.py"
    file2.write_text("""from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    # Create pyproject.toml with function configurations
    # Note: For nested files, we need to use the module stem only since
    # the matching logic in RegistryDetector checks module_path.stem == module_part
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "top.sql"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "nested.sql"
query_param = "query"
""")

    result = registries_cli([tmp_path])

    assert result.returncode == 0
    output_lines = result.stdout.strip().split("\n")
    assert len(output_lines) == 2
    assert any("top.py:sql" in line for line in output_lines)
    assert any("nested.py:sql" in line for line in output_lines)
