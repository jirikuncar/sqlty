"""Tests for the sqlty.registries subcommand."""

import subprocess
import sys
from pathlib import Path


def test_registries_find_single_file(tmp_path: Path) -> None:
    """Test finding registries in a single file."""
    # Create a test file with a registry
    test_file = tmp_path / "test.py"
    test_file.write_text("""from sqlty import SQLRegistry

class TestRegistry(SQLRegistry):
    pass
""")

    # Run the registries command
    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", str(test_file)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    assert str(test_file) + ":TestRegistry" in result.stdout


def test_registries_find_in_directory(tmp_path: Path) -> None:
    """Test finding registries in a directory."""
    # Create multiple files with registries
    file1 = tmp_path / "queries1.py"
    file1.write_text("""from sqlty import SQLRegistry

class QueryA(SQLRegistry):
    pass
""")

    file2 = tmp_path / "queries2.py"
    file2.write_text("""from sqlty import SQLRegistry

class QueryB(SQLRegistry):
    pass

class QueryC(SQLRegistry):
    pass
""")

    # Run the registries command
    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    output_lines = result.stdout.strip().split("\n")
    assert len(output_lines) == 3
    assert any("queries1.py:QueryA" in line for line in output_lines)
    assert any("queries2.py:QueryB" in line for line in output_lines)
    assert any("queries2.py:QueryC" in line for line in output_lines)


def test_registries_indirect_import(tmp_path: Path) -> None:
    """Test finding registries with indirect imports (module.Class)."""
    test_file = tmp_path / "test.py"
    test_file.write_text("""import sqlty

class IndirectRegistry(sqlty.SQLRegistry):
    pass
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", str(test_file)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    assert str(test_file) + ":IndirectRegistry" in result.stdout


def test_registries_default_path(tmp_path: Path) -> None:
    """Test using default path (current directory)."""
    test_file = tmp_path / "test.py"
    test_file.write_text("""from sqlty import SQLRegistry

class DefaultRegistry(SQLRegistry):
    pass
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0
    assert "test.py:DefaultRegistry" in result.stdout


def test_registries_nonexistent_path() -> None:
    """Test error handling for nonexistent path."""
    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", "/nonexistent/path"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 1
    assert "does not exist" in result.stderr


def test_registries_too_many_args() -> None:
    """Test error handling for too many arguments."""
    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", "arg1", "arg2"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 1
    assert "Usage" in result.stderr


def test_registries_no_registries(tmp_path: Path) -> None:
    """Test when no registries are found."""
    test_file = tmp_path / "test.py"
    test_file.write_text("""# Just a regular Python file
def some_function():
    pass
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    assert result.stdout.strip() == ""


def test_registries_nested_directories(tmp_path: Path) -> None:
    """Test finding registries in nested directories."""
    # Create nested structure
    subdir = tmp_path / "subdir"
    subdir.mkdir()

    file1 = tmp_path / "top.py"
    file1.write_text("""from sqlty import SQLRegistry

class TopRegistry(SQLRegistry):
    pass
""")

    file2 = subdir / "nested.py"
    file2.write_text("""from sqlty import SQLRegistry

class NestedRegistry(SQLRegistry):
    pass
""")

    result = subprocess.run(
        [sys.executable, "-m", "sqlty.registries", str(tmp_path)],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0
    output_lines = result.stdout.strip().split("\n")
    assert len(output_lines) == 2
    assert any("top.py:TopRegistry" in line for line in output_lines)
    assert any("nested.py:NestedRegistry" in line for line in output_lines)
