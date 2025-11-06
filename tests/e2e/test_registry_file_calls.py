"""Test that sql calls in the same file as SQLRegistry definition are detected.

This test validates the scenario described in the issue:
- SQLRegistry subclass is defined in a file
- sql = Registry.sql alias is created in the same file  
- sql() calls are made in the same file

All calls should be properly detected and typed.

The implementation in _collector.py handles this through:
1. visit_ClassDef: Tracks SQLRegistry subclasses in local_registry_classes
2. visit_Assign: Tracks assignments like "sql = Registry.sql" in registry_aliases
3. visit_Call: Detects calls using tracked aliases and associates them with the correct registry

This feature is already demonstrated in examples/multi_schema/orders_queries.py.
"""

from pathlib import Path

from inline_snapshot import snapshot


def test_sql_call_in_registry_file(tmp_path: Path, cli) -> None:
    """Test that sql calls in the same file as registry definition are detected."""
    # Create schema file
    schema_file = tmp_path / "orders.sql"
    schema_file.write_text("""CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    total FLOAT NOT NULL
);
""")

    # Create registry file with sql calls in the same file
    registry_file = tmp_path / "orders_queries.py"
    registry_file.write_text("""from sqlty import SQLRegistry


class OrdersQueries(SQLRegistry):
    __schema_path__ = "orders.sql"


sql = OrdersQueries.sql

# This should be detected and typed
order_query = sql("SELECT id, total FROM orders")
""")

    # Generate stubs
    result = cli([str(tmp_path), "--mode=full"])

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "orders_queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Should infer correct types from schema: id -> int, total -> float
    assert "tuple[int, float]" in stub_content
    assert "SELECT id, total FROM orders" in stub_content

    # Snapshot the full result
    assert stub_content == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class OrdersQueries(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id, total FROM orders']) -> SQL[tuple[int, float]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = OrdersQueries.sql
''')


def test_multiple_aliases_in_registry_file(tmp_path: Path, cli) -> None:
    """Test that multiple aliases to sql method are all detected."""
    # Create schema file
    schema_file = tmp_path / "orders.sql"
    schema_file.write_text("""CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    total FLOAT NOT NULL
);
""")

    # Create registry file with multiple aliases
    registry_file = tmp_path / "orders_queries.py"
    registry_file.write_text("""from sqlty import SQLRegistry


class OrdersQueries(SQLRegistry):
    __schema_path__ = "orders.sql"


# Multiple aliases
sql = OrdersQueries.sql
query = OrdersQueries.sql
order_sql = OrdersQueries.sql

# Calls using different aliases
q1 = sql("SELECT id, total FROM orders WHERE id = 1")
q2 = query("SELECT id, total FROM orders WHERE id = 2")
q3 = order_sql("SELECT id, total FROM orders WHERE id = 3")

# Direct call
q4 = OrdersQueries.sql("SELECT id, total FROM orders WHERE id = 4")
""")

    # Generate stubs
    result = cli([str(tmp_path), "--mode=full"])

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "orders_queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # All 4 queries should be present
    assert "WHERE id = 1" in stub_content
    assert "WHERE id = 2" in stub_content
    assert "WHERE id = 3" in stub_content
    assert "WHERE id = 4" in stub_content

    # All should have correct type
    assert stub_content.count("tuple[int, float]") >= 4


def test_multiple_registries_in_same_file(tmp_path: Path, cli) -> None:
    """Test that multiple registry classes in same file are all handled."""
    # Create schema file
    schema_file = tmp_path / "schema.sql"
    schema_file.write_text("""CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    total FLOAT NOT NULL
);
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);
""")

    # Create file with multiple registries
    registry_file = tmp_path / "queries.py"
    registry_file.write_text("""from sqlty import SQLRegistry


class OrderQueries(SQLRegistry):
    __schema_path__ = "schema.sql"


class UserQueries(SQLRegistry):
    __schema_path__ = "schema.sql"


# Aliases for each registry
order_sql = OrderQueries.sql
user_sql = UserQueries.sql

# Calls using different registries
order = order_sql("SELECT id, total FROM orders")
user = user_sql("SELECT id, name FROM users")

# Direct calls
order2 = OrderQueries.sql("SELECT total FROM orders WHERE id = 1")
user2 = UserQueries.sql("SELECT name FROM users WHERE id = 1")
""")

    # Generate stubs
    result = cli([str(tmp_path), "--mode=full"])

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Both registry classes should be present
    assert "class OrderQueries(SQLRegistry):" in stub_content
    assert "class UserQueries(SQLRegistry):" in stub_content

    # Orders queries should be in OrderQueries
    assert "SELECT id, total FROM orders" in stub_content
    assert "SELECT total FROM orders WHERE id = 1" in stub_content

    # Users queries should be in UserQueries
    assert "SELECT id, name FROM users" in stub_content
    assert "SELECT name FROM users WHERE id = 1" in stub_content

    # Check types
    assert "tuple[int, float]" in stub_content  # orders: id, total
    assert "tuple[int, str]" in stub_content  # users: id, name
