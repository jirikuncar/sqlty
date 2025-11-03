from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_multiple_schema_mapping(tmp_path: Path) -> None:
    # Create users schema
    users_schema = tmp_path / "users.sql"
    users_schema.write_text("""
    CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL
    );
    """)
    # Create orders schema
    orders_schema = tmp_path / "orders.sql"
    orders_schema.write_text("""
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        total FLOAT NOT NULL
    );
    """)
    # Create users registry
    users_file = tmp_path / "users_queries.py"
    users_file.write_text("""
from sqlty import SQLRegistry
class UsersQueries(SQLRegistry):
    __schema_path__ = "users.sql"
sql = UsersQueries.sql
user_query = sql("SELECT id, name FROM users")
""")
    # Create orders registry
    orders_file = tmp_path / "orders_queries.py"
    orders_file.write_text("""
from sqlty import SQLRegistry
class OrdersQueries(SQLRegistry):
    __schema_path__ = "orders.sql"
sql = OrdersQueries.sql
order_query = sql("SELECT id, total FROM orders")
""")
    # Multiple schema mapping
    schema_mappings = {
        "users_queries": users_schema,
        "orders_queries": orders_schema,
    }
    # Generate stubs for both
    generator = TypeStubGenerator(sql_dialect="postgres", schema_mappings=schema_mappings)
    users_stub = generator.generate_stub_file(users_file, mode="full")
    orders_stub = generator.generate_stub_file(orders_file, mode="full")
    # Check types
    assert "SQL[tuple[int, str]]" in Path(users_stub).read_text()
    assert "SQL[tuple[int, float]]" in Path(orders_stub).read_text()
