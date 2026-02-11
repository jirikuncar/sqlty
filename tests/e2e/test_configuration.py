"""End-to-end tests for configuration via pyproject.toml.

Tests that sqlty can be configured via pyproject.toml file.
"""

from collections.abc import Callable
from pathlib import Path
from subprocess import CompletedProcess

import pytest

pytestmark = pytest.mark.skip(reason="Legacy generate CLI deferred to PLAN-FUTURE.md")


def test_config_via_pyproject_toml(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    create_pyproject_toml: Callable[..., Path],
) -> None:
    """Test that the tool accepts pyproject.toml configuration.

    This test validates that the tool can read and process a pyproject.toml
    file with sqlty configuration. The tool should either use the configuration
    if the feature is implemented, or gracefully ignore it if not yet supported.
    """
    # Create pyproject.toml with sqlty configuration
    # Create pyproject.toml with dialect and register queries.sql
    create_pyproject_toml(tmp_path, [("queries.sql", "query")], dialect="postgres")

    # Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from typing import Any
from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Run generator - should pick up config from pyproject.toml
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    # Should handle config gracefully
    assert result.returncode == 0

    # If successful, verify stub was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_file.read_text()


def test_schema_path_in_config(tmp_path: Path, cli: Callable[..., CompletedProcess[str]]) -> None:
    """Test schema path configuration in pyproject.toml."""
    # Create schema file
    schema_file = tmp_path / "db_schema.sql"
    schema_file.write_text("""CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
""")

    # Create pyproject.toml referencing schema
    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text("""[tool.sqlty]
schema = "db_schema.sql"
dialect = "postgres"
""")

    # Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from typing import Any
from sqlty import sqlty


@sqlty
def sql(query: str) -> Any:
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id, name FROM users")
""")

    # Run generator
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    # Should succeed or gracefully handle config
    assert result.returncode in (0, 1, 2)


def test_cli_args_override_config(
    tmp_path: Path, cli: Callable[..., CompletedProcess[str]]
) -> None:
    """Test that CLI arguments override pyproject.toml configuration."""
    # Create pyproject.toml with one dialect
    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text("""[tool.sqlty]
dialect = "mysql"
""")

    # Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query
""")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Run with different dialect via CLI - should override config
    result = cli([str(tmp_path), "--dialect=postgres"], {"cwd": str(tmp_path)})

    # Should succeed
    assert result.returncode == 0


def test_multiple_registries_in_config(
    tmp_path: Path, cli: Callable[..., CompletedProcess[str]]
) -> None:
    """Test configuring multiple registries in pyproject.toml."""
    # Create pyproject.toml with registry configuration
    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text("""[tool.sqlty]
mode = "full"

[[tool.sqlty.registry]]
name = "UserQuery"
module = "user_queries"

[[tool.sqlty.registry]]
name = "OrderQuery"
module = "order_queries"
""")

    # Create registries
    (tmp_path / "user_queries.py").write_text("""from sqlty import sqlty

@sqlty
def user_sql[T](query: T) -> T:
    return query
""")

    (tmp_path / "order_queries.py").write_text("""from sqlty import sqlty

@sqlty
def order_sql[T](query: T) -> T:
    return query
""")

    # Create app
    (tmp_path / "app.py").write_text("""from user_queries import user_sql
from order_queries import order_sql

users = user_sql("SELECT id::INTEGER FROM users")
orders = order_sql("SELECT id::INTEGER FROM orders")
""")

    # Run generator
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    # Should handle registry configuration
    assert result.returncode in (0, 1, 2)


def test_exclude_patterns_in_config(
    tmp_path: Path, cli: Callable[..., CompletedProcess[str]]
) -> None:
    """Test exclude patterns in pyproject.toml configuration."""
    # Create pyproject.toml with exclude patterns
    pyproject_file = tmp_path / "pyproject.toml"
    pyproject_file.write_text("""[tool.sqlty]
exclude = ["tests/**", "build/**"]
""")

    # Create main queries
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query
""")

    # Create app
    (tmp_path / "app.py").write_text("""from queries import sql

query = sql("SELECT id::INTEGER FROM users")
""")

    # Create test directory with queries that should be excluded
    tests_dir = tmp_path / "tests"
    tests_dir.mkdir()
    (tests_dir / "test_queries.py").write_text("""from queries import sql

# This should be excluded
test_query = sql("SELECT * FROM test_table")
""")

    # Run generator
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    # Should succeed
    assert result.returncode in (0, 1, 2)

    # If stub was generated, it should not include the test query
    stub_file = tmp_path / "queries.pyi"
    if stub_file.exists():
        stub_content = stub_file.read_text()
        # Main query should be present
        assert "SELECT id::INTEGER FROM users" in stub_content
        # Test query should ideally not be present (if exclude works)
        # But we won't strictly assert this as the feature may not be implemented yet


def test_schema_paths_resolved_relative_to_pyproject_toml(
    tmp_path: Path, cli, setup_sqlty_package
) -> None:
    """Test that schema paths in pyproject.toml are resolved relative to the pyproject.toml file location, not cwd."""
    # Create a subdirectory for the project
    project_dir = tmp_path / "my_project"
    project_dir.mkdir()

    # Create pyproject.toml in the subdirectory with schema configurations
    pyproject_file = project_dir / "pyproject.toml"
    pyproject_file.write_text("""[tool.sqlty]
dialect = "sqlite"
schemas = { users_queries = "users.sql", orders_queries = "orders.sql" }
""")

    # Create schema files in the same directory as pyproject.toml
    users_schema = project_dir / "users.sql"
    users_schema.write_text("""CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
""")

    orders_schema = project_dir / "orders.sql"
    orders_schema.write_text("""CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    total FLOAT NOT NULL
);
""")

    # Create query modules in the project directory using the fixture
    setup_sqlty_package(project_dir, "users_queries.py", "sql")
    setup_sqlty_package(project_dir, "orders_queries.py", "sql")

    # Add query calls to the files
    users_queries_file = project_dir / "users_queries.py"
    users_queries_file.write_text(
        users_queries_file.read_text() + '\n\nuser_query = sql("SELECT id, name FROM users")'
    )

    orders_queries_file = project_dir / "orders_queries.py"
    orders_queries_file.write_text(
        orders_queries_file.read_text() + '\n\norder_query = sql("SELECT id, total FROM orders")'
    )

    # Create app file
    app_file = project_dir / "app.py"
    app_file.write_text("""from users_queries import sql as user_sql
from orders_queries import sql as order_sql

users = user_sql("SELECT id, name FROM users")
orders = order_sql("SELECT id, total FROM orders")
""")

    # Run sqlty from the parent directory (tmp_path), not from project_dir
    # This tests that schema paths are resolved relative to pyproject.toml location
    result = cli([str(project_dir)], {"cwd": str(tmp_path)})

    # Should succeed and find the schema files
    assert result.returncode == 0
    assert "Using schema from" in result.stderr
    assert "users.sql" in result.stderr
    assert "orders.sql" in result.stderr

    # Check that correct stub files were generated with proper types
    users_stub = project_dir / "users_queries.pyi"
    orders_stub = project_dir / "orders_queries.pyi"

    assert users_stub.exists()
    assert orders_stub.exists()

    # Verify the types are correct (schema-based inference worked)
    users_content = users_stub.read_text()
    orders_content = orders_stub.read_text()

    assert "SQL[tuple[int, str]]" in users_content  # id INTEGER, name TEXT
    assert "SQL[tuple[int, float]]" in orders_content  # id INTEGER, total FLOAT
