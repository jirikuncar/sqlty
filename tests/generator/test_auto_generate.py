"""Tests for automatic stub file generation at runtime."""

import subprocess
import sys
from pathlib import Path


def test_auto_generate_disabled_by_default(tmp_path: Path) -> None:
    """Test that auto-generation is disabled by default."""
    # Create a temporary module file
    module_path = tmp_path / "test_registry.py"
    module_path.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

query = sql("SELECT 1")
""")

    stub_path = module_path.with_suffix(".pyi")

    # Run the module in subprocess (should not generate stub)
    result = subprocess.run(
        [sys.executable, str(module_path)],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0

    # Stub should NOT have been generated
    assert not stub_path.exists()


def test_auto_generate_with_env_var(tmp_path: Path) -> None:
    """Test that auto-generation works when SQLTY_AUTO_GENERATE is set."""
    # Create a temporary module file
    module_path = tmp_path / "test_auto_gen.py"
    module_path.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

query1 = sql("SELECT id, name FROM users")
query2 = sql("SELECT COUNT(*) FROM posts")
""")

    stub_path = module_path.with_suffix(".pyi")

    # Run the module in subprocess with environment variable
    result = subprocess.run(
        [sys.executable, str(module_path)],
        capture_output=True,
        text=True,
        env={"SQLTY_AUTO_GENERATE": "1"},
    )
    assert result.returncode == 0

    # Stub should have been generated
    assert stub_path.exists()

    # Check stub content
    content = stub_path.read_text()
    assert "@overload" in content
    assert "SELECT id, name FROM users" in content
    assert "SELECT COUNT(*) FROM posts" in content


def test_auto_generate_incremental(tmp_path: Path) -> None:
    """Test that auto-generation creates stubs when they don't exist."""
    module_path = tmp_path / "test_incremental.py"
    stub_path = module_path.with_suffix(".pyi")

    # Create module that generates a stub
    module_path.write_text("""
from typing import Any
from sqlty import sqlty

@sqlty
def sql(query: str) -> Any:
    return query

query = sql("SELECT new_query FROM table")
""")

    subprocess.run(
        [sys.executable, str(module_path)],
        check=True,
        env={"SQLTY_AUTO_GENERATE": "true"},
    )

    # Stub should have been generated
    assert stub_path.exists()

    # Check that it contains the query
    content = stub_path.read_text()
    assert "SELECT new_query FROM table" in content


def test_auto_generate_with_different_values(tmp_path: Path) -> None:
    """Test that various truthy values enable auto-generation."""
    test_values = ["1", "true", "TRUE", "yes", "YES"]

    for value in test_values:
        # Create a unique subdirectory for each test value
        test_dir = tmp_path / f"test_{value}"
        test_dir.mkdir(exist_ok=True)

        module_path = test_dir / f"test_value_{value}.py"
        module_path.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

query = sql("SELECT 1")
""")

        stub_path = module_path.with_suffix(".pyi")

        # Run the module in subprocess with environment variable
        result = subprocess.run(
            [sys.executable, str(module_path)],
            capture_output=True,
            text=True,
            env={"SQLTY_AUTO_GENERATE": value},
        )
        assert result.returncode == 0

        # Stub should have been generated
        assert stub_path.exists(), f"Stub not generated for value '{value}'"


def test_auto_generate_error_handling(tmp_path: Path) -> None:
    """Test that errors during auto-generation don't crash the application."""
    # We test error handling by ensuring that even with auto-generate enabled,
    # a module without sql calls can still be imported successfully
    module_path = tmp_path / "test_no_sql.py"
    module_path.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query

# No sql() calls, so no stub should be generated
""")

    stub_path = module_path.with_suffix(".pyi")

    # Run the module in subprocess with environment variable
    result = subprocess.run(
        [sys.executable, str(module_path)],
        capture_output=True,
        text=True,
        env={"SQLTY_AUTO_GENERATE": "1"},
    )
    assert result.returncode == 0

    # Module should still be importable (subprocess succeeded)
    # No stub should be generated since there were no sql() calls
    assert not stub_path.exists()
