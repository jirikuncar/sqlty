"""Tests for automatic stub file generation at runtime."""

import importlib.util
import sys
from pathlib import Path

import pytest


def test_auto_generate_disabled_by_default(tmp_path: Path) -> None:
    """Test that auto-generation is disabled by default."""
    # Create a temporary module file
    module_path = tmp_path / "test_registry.py"
    module_path.write_text("""
from sqlty import SQLRegistry

class MyQuery(SQLRegistry):
    pass

sql = MyQuery.sql
query = sql("SELECT 1")
""")

    stub_path = module_path.with_suffix(".pyi")

    # Import the module (should not generate stub)
    sys.path.insert(0, str(tmp_path))
    try:
        import test_registry  # type: ignore[import-not-found] # noqa: F401

        # Stub should NOT have been generated
        assert not stub_path.exists()
    finally:
        sys.path.remove(str(tmp_path))
        if "test_registry" in sys.modules:
            del sys.modules["test_registry"]


def test_auto_generate_with_env_var(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that auto-generation works when SQLTY_AUTO_GENERATE is set."""
    # Create a temporary module file
    module_path = tmp_path / "test_auto_gen.py"
    module_path.write_text("""
from sqlty import SQLRegistry

class AutoQuery(SQLRegistry):
    pass

sql = AutoQuery.sql
query1 = sql("SELECT id, name FROM users")
query2 = sql("SELECT COUNT(*) FROM posts")
""")

    stub_path = module_path.with_suffix(".pyi")

    # Set environment variable
    monkeypatch.setenv("SQLTY_AUTO_GENERATE", "1")

    sys.path.insert(0, str(tmp_path))
    try:
        import test_auto_gen  # type: ignore[import-not-found] # noqa: F401

        # Stub should have been generated
        assert stub_path.exists()

        # Check stub content
        content = stub_path.read_text()
        assert "class AutoQuery(SQLRegistry):" in content
        assert "SELECT id, name FROM users" in content
        assert "SELECT COUNT(*) FROM posts" in content

    finally:
        sys.path.remove(str(tmp_path))
        if "test_auto_gen" in sys.modules:
            del sys.modules["test_auto_gen"]


def test_auto_generate_incremental(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that auto-generation uses incremental mode (doesn't overwrite)."""
    module_path = tmp_path / "test_incremental.py"
    stub_path = module_path.with_suffix(".pyi")

    # Create an existing stub with custom content
    existing_stub = """# Custom stub content
from typing import overload

class IncrementalQuery:
    @overload
    @classmethod
    def sql(cls, query: str) -> str: ...
"""
    stub_path.write_text(existing_stub)

    # Create module that would generate a different stub
    module_path.write_text("""
from sqlty import SQLRegistry

class IncrementalQuery(SQLRegistry):
    pass

sql = IncrementalQuery.sql
query = sql("SELECT new_query FROM table")
""")

    # Set environment variable
    monkeypatch.setenv("SQLTY_AUTO_GENERATE", "true")

    sys.path.insert(0, str(tmp_path))
    try:
        import test_incremental  # type: ignore[import-not-found] # noqa: F401

        # Stub should still exist
        assert stub_path.exists()

        # Check that it was updated incrementally (new query added)
        content = stub_path.read_text()
        # In incremental mode, the new query should be added
        assert "SELECT new_query FROM table" in content

    finally:
        sys.path.remove(str(tmp_path))
        if "test_incremental" in sys.modules:
            del sys.modules["test_incremental"]


def test_auto_generate_with_different_values(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Test that various truthy values enable auto-generation."""
    test_values = ["1", "true", "TRUE", "yes", "YES"]

    for value in test_values:
        # Create a unique subdirectory for each test value
        test_dir = tmp_path / f"test_{value}"
        test_dir.mkdir(exist_ok=True)

        module_path = test_dir / f"test_value_{value}.py"
        module_path.write_text(f"""
from sqlty import SQLRegistry

class Query{value}(SQLRegistry):
    pass

sql = Query{value}.sql
query = sql("SELECT 1")
""")

        stub_path = module_path.with_suffix(".pyi")

        # Set environment variable
        monkeypatch.setenv("SQLTY_AUTO_GENERATE", value)

        sys.path.insert(0, str(test_dir))
        try:
            # Use importlib to dynamically load the module
            module_name = f"test_value_{value}"
            spec = importlib.util.spec_from_file_location(module_name, module_path)
            if spec and spec.loader:
                module = importlib.util.module_from_spec(spec)
                sys.modules[module_name] = module
                spec.loader.exec_module(module)

            # Stub should have been generated
            assert stub_path.exists(), f"Stub not generated for value '{value}'"

        finally:
            sys.path.remove(str(test_dir))
            module_name_to_clean = f"test_value_{value}"
            if module_name_to_clean in sys.modules:
                del sys.modules[module_name_to_clean]


def test_auto_generate_error_handling(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Test that errors during auto-generation don't crash the application."""
    # We test error handling by ensuring that even with auto-generate enabled,
    # a module without sql calls can still be imported successfully
    module_path = tmp_path / "test_no_sql.py"
    module_path.write_text("""
from sqlty import SQLRegistry

class NoSQLQuery(SQLRegistry):
    # No sql() calls, which should generate a minimal stub
    pass
""")

    stub_path = module_path.with_suffix(".pyi")

    # Set environment variable
    monkeypatch.setenv("SQLTY_AUTO_GENERATE", "1")

    sys.path.insert(0, str(tmp_path))
    try:
        # This should not crash even if there are no sql calls
        import test_no_sql  # type: ignore[import-not-found] # noqa: F401

        # Module should still be importable
        assert "test_no_sql" in sys.modules

        # Stub should still have been generated (minimal stub)
        assert stub_path.exists()
        content = stub_path.read_text()
        assert "class NoSQLQuery(SQLRegistry):" in content

    finally:
        sys.path.remove(str(tmp_path))
        if "test_no_sql" in sys.modules:
            del sys.modules["test_no_sql"]
