"""Test that local sql() functions not decorated with @sqlty are ignored."""

import subprocess
from collections.abc import Callable
from pathlib import Path


def test_local_sql_function_ignored(
    tmp_path: Path, cli: Callable[..., subprocess.CompletedProcess[str]]
) -> None:
    """Test that a local sql() function is ignored if not decorated with @sqlty."""
    # Create a file with a local sql() function (NOT decorated with @sqlty)
    local_file = tmp_path / "local_helper.py"
    local_file.write_text("""
def sql(query: str) -> str:
    '''A local helper function.'''
    return f"PROCESSED: {query}"
""")

    # Create a file that imports and uses this local sql()
    app_file = tmp_path / "app.py"
    app_file.write_text("""
from local_helper import sql

# This should NOT generate a stub
result = sql("SELECT * FROM users")
""")

    # Run the generator
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check that NO stub was generated for local_helper
    stub_file = tmp_path / "local_helper.pyi"
    assert not stub_file.exists(), "Stub should not be generated for local sql() function"


def test_registry_sql_function_generates_stub(
    tmp_path: Path, cli: Callable[..., subprocess.CompletedProcess[str]]
) -> None:
    """Test that sql() from @sqlty decorated function is correctly processed."""
    # Create a registry file
    registry_file = tmp_path / "queries.py"
    registry_file.write_text("""
from sqlty import sqlty

@sqlty
def sql(query: str):
    return query
""")

    # Create a file that imports and uses the registry sql()
    app_file = tmp_path / "app.py"
    app_file.write_text("""
from queries import sql

# This SHOULD generate a stub
result = sql("SELECT id::INTEGER FROM users")
""")

    # Run the generator
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check that a stub WAS generated for queries
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists(), "Stub should be generated for @sqlty decorated sql() function"

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in stub_content, "Query should be in stub"
    assert "@overload" in stub_content, "Overloads should be in stub"


def test_multi_registry_with_aliases(
    tmp_path: Path, cli: Callable[..., subprocess.CompletedProcess[str]]
) -> None:
    """Test the example from issue: multiple registries with aliases."""
    # Create query.py with two registries
    query_file = tmp_path / "query.py"
    query_file.write_text("""
from sqlty import sqlty

@sqlty
def qa(query: str):
    return query

@sqlty
def qb(query: str):
    return query
""")

    # Create app.py that uses both registries
    app_file = tmp_path / "app.py"
    app_file.write_text("""
from typing import TYPE_CHECKING
from query import qa, qb

if TYPE_CHECKING:
    from sqlty import SQL

a: SQL[tuple[int]] = qa("SELECT 1::INTEGER")
b: SQL[tuple[str]] = qb("SELECT 'a'::TEXT")
""")

    # Run the generator
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check that a stub was generated with both classes
    stub_file = tmp_path / "query.pyi"
    assert stub_file.exists(), "Stub should be generated for query.py"

    stub_content = stub_file.read_text()
    assert "@overload" in stub_content, "Overloads should be in stub"
    assert "SELECT 1::INTEGER" in stub_content, "Query for qa should be in stub"
    assert "SELECT 'a'::TEXT" in stub_content, "Query for qb should be in stub"


def test_import_registry_class_directly(
    tmp_path: Path, cli: Callable[..., subprocess.CompletedProcess[str]]
) -> None:
    """Test importing and using registry class directly (from issue example variant)."""
    # Create query.py with two registries
    query_file = tmp_path / "query.py"
    query_file.write_text("""
from sqlty import sqlty

@sqlty
def qa(query: str):
    return query

@sqlty
def qb(query: str):
    return query
""")

    # Create app.py that imports functions directly
    app_file = tmp_path / "app.py"
    app_file.write_text("""
from typing import TYPE_CHECKING
from query import qa, qb

if TYPE_CHECKING:
    from sqlty import SQL

a: SQL[tuple[int]] = qa("SELECT 2::INTEGER")
b: SQL[tuple[str]] = qb("SELECT 'b'::TEXT")
""")

    # Run the generator
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Check that a stub was generated with both classes
    stub_file = tmp_path / "query.pyi"
    assert stub_file.exists(), "Stub should be generated for query.py"

    stub_content = stub_file.read_text()
    assert "@overload" in stub_content, "Overloads should be in stub"
    # These queries are in app.py but should be in query.pyi since they reference query module functions
    assert "SELECT 2::INTEGER" in stub_content, "Query for qa should be in stub"
    assert "SELECT 'b'::TEXT" in stub_content, "Query for qb should be in stub"
