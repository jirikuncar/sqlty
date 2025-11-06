"""End-to-end tests validating mypy type checking on generated stubs.

These tests ensure that the generated stubs work correctly with mypy
and that type annotations are properly inferred and validated.
"""

from pathlib import Path


def test_mypy_validates_query_types(
    tmp_path: Path, cli, mypy, stubtest, setup_queries_file
) -> None:
    """Test that mypy validates query return types correctly."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from psycopg import Connection
from psycopg.abc import Params
from sqlty.drivers.psycopg import execute

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

q: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")

conn = Connection.connect("dbname=test user=postgres password=secret host=localhost port=5432")

user_id: int
user_name: str

for user_id, user_name in execute(conn, q, {}):
    print(f"User {user_id}: {user_name}")
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    mypy_result = mypy(
        [str(app_file), "--strict"],
    )
    assert mypy_result.returncode == 0, mypy_result.stdout

    # Verify with stubtest
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_mypy_catches_type_mismatch(tmp_path: Path, cli, mypy, setup_queries_file) -> None:
    """Test that mypy catches type mismatches in query usage."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

q: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")
wrong: SQL[tuple[str, int]] = q
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    mypy_result = mypy([str(app_file), "--strict"])
    assert mypy_result.returncode != 0
    assert "incompatible" in mypy_result.stdout.lower() or "error" in mypy_result.stdout.lower()


def test_mypy_infers_tuple_unpacking(tmp_path: Path, cli, mypy, setup_queries_file) -> None:
    """Test that mypy correctly infers types from tuple unpacking with execute function."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from collections.abc import Iterator
    from psycopg import Connection
    from sqlty import SQL
    from sqlty.drivers.psycopg import execute

query: SQL[tuple[int, str, str]] = sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")

def process_user(user_id: int, name: str, email: str) -> None:
    print(f"{user_id}: {name} <{email}>")

if TYPE_CHECKING:
    conn: Connection
    results: Iterator[tuple[int, str, str]] = execute(conn, query, {})
    for user_id, name, email in results:
        process_user(user_id, name, email)
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    mypy_result = mypy([str(app_file), "--strict"])
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_mypy_with_multiple_queries(
    tmp_path: Path, cli, mypy, stubtest, setup_queries_file
) -> None:
    """Test mypy validation with multiple queries of different types."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Different query types
int_query: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM users")
str_query: SQL[tuple[str]] = sql("SELECT name::TEXT FROM users LIMIT 1")
multi_query: SQL[tuple[int, str, str]] = sql(
    "SELECT id::INTEGER, name::TEXT, email::TEXT FROM users"
)

# Verify types are distinct
def check_types() -> None:
    # These should all type-check correctly
    i: SQL[tuple[int]] = int_query
    s: SQL[tuple[str]] = str_query
    m: SQL[tuple[int, str, str]] = multi_query
""")

    # Generate stub
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Verify stub contains all query types
    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert "SQL[tuple[int]]" in stub_content
    assert "SQL[tuple[str]]" in stub_content
    assert "SQL[tuple[int, str, str]]" in stub_content

    # Run mypy
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"

    # Verify with stubtest
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_mypy_with_query_parameters(tmp_path: Path, cli, mypy, setup_queries_file) -> None:
    """Test mypy validation for queries with parameters."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query: SQL[tuple[int, str]] = sql(
    "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :user_id"
)
result_type: SQL[tuple[int, str]] = query
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert ":user_id" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content

    mypy_result = mypy([str(app_file), "--strict"])
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_mypy_with_complex_types(tmp_path: Path, cli, mypy, setup_queries_file) -> None:
    """Test mypy validation with complex SQL types like arrays or JSON."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query: SQL[tuple[int, str, str]] = sql(
    "SELECT id::INTEGER, data::TEXT, tags::TEXT FROM items"
)
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    mypy_result = mypy([str(app_file), "--strict"])
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_stubtest_detects_stub_inconsistencies(
    tmp_path: Path, cli, stubtest, setup_queries_file
) -> None:
    """Test that stubtest detects inconsistencies between stub and implementation."""
    setup_queries_file(tmp_path)

    # Create an app file with queries so stubs are generated
    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Generate stub
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Manually modify the stub to introduce an inconsistency
    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Add a method that doesn't exist in the runtime
    modified_stub = stub_content.replace(
        "class Query(SQLRegistry):",
        """class Query(SQLRegistry):
    def nonexistent_method(self) -> None: ...""",
    )
    stub_file.write_text(modified_stub)

    # Run stubtest - it should detect the inconsistency
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    # This should fail because we added a non-existent method
    assert stubtest_result.returncode != 0
    assert "nonexistent_method" in stubtest_result.stdout


def test_stubtest_with_allowlist(tmp_path: Path, cli, stubtest, setup_queries_file) -> None:
    """Test that stubtest allowlist is properly applied."""
    setup_queries_file(tmp_path)

    # Create a file with runtime-only elements that should be in the allowlist
    queries_file = tmp_path / "queries.py"
    queries_file.write_text("""from sqlty import SQLRegistry


class Query(SQLRegistry):
    __schema_path__ = "schema.sql"  # This should be allowed by allowlist
    pass


sql = Query.sql

# Runtime query variable - should be allowed by allowlist
user_query = sql("SELECT id::INTEGER, name::TEXT FROM users")


def main() -> None:  # Should be allowed by allowlist
    print("Hello")
""")

    # Generate stub
    result = cli([str(tmp_path)])
    assert result.returncode == 0

    # Run stubtest - should pass because allowlist covers these cases
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}, "cwd": str(tmp_path)},
    )
    assert stubtest_result.returncode == 0, (
        f"stubtest should pass with allowlist: {stubtest_result.stdout}"
    )
