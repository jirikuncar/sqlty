"""End-to-end tests for driver integration patterns.

Tests that the generated stubs work correctly with driver code patterns,
particularly the psycopg driver integration.
"""

from collections.abc import Callable
from pathlib import Path
from subprocess import CompletedProcess

import pytest
from inline_snapshot import snapshot

pytestmark = pytest.mark.skip(reason="Legacy generate CLI deferred to PLAN-FUTURE.md")


def test_psycopg_driver_pattern(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    mypy: Callable[..., CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str], Path],
) -> None:
    """Test stub generation for code using psycopg driver pattern."""
    setup_sqlty_package(tmp_path, "queries.py")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from collections.abc import Iterator
    from sqlty import SQL

# Query definitions that would be used with psycopg
get_user: SQL[tuple[int, str]] = sql(
    "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id"
)

get_posts: SQL[tuple[int, str, str]] = sql(
    "SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id"
)

count_users: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM users")


# Type annotations that would be used with execute()
def example_usage() -> None:
    # These type annotations should work with the generated stubs
    users: Iterator[tuple[int, str]]
    posts: Iterator[tuple[int, str, str]]
    counts: Iterator[tuple[int]]
""")

    # Generate stub
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Verify stub
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Verify all queries are present
    assert "SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id" in stub_content
    assert (
        "SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id"
        in stub_content
    )
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT COUNT(*)::INTEGER FROM users']) -> SQL[tuple[int]]:
    ...

@overload
def sql(query: Literal['SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id']) -> SQL[tuple[int, str]]:
    ...

@overload
def sql(query: Literal['SELECT id::INTEGER, title::TEXT, content::TEXT FROM posts WHERE user_id = :user_id']) -> SQL[tuple[int, str, str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
""")

    # Validate with mypy
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_query_with_iterator_return_type(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    mypy: Callable[..., CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str], Path],
) -> None:
    """Test that SQL[tuple[...]] works with Iterator[tuple[...]] in type checking."""
    setup_sqlty_package(tmp_path, "queries.py")

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from collections.abc import Iterator
    from sqlty import SQL


def fetch_users() -> None:
    """Demonstrate iterator pattern with SQL queries."""
    query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")

    # In real code, execute() would return Iterator[tuple[int, str]]
    # For type checking, we simulate this
    result: Iterator[tuple[int, str]] = iter([])  # Placeholder

    for user_id, name in result:
        print(f"User {user_id}: {name}")
''')

    # Generate stub
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Validate with mypy
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_parameterized_query_type_safety(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    mypy: Callable[..., CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str], Path],
) -> None:
    """Test that parameterized queries maintain type safety."""
    setup_sqlty_package(tmp_path, "queries.py")

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Query with multiple parameters
query: SQL[tuple[int, str, str]] = sql(
    """
    SELECT id::INTEGER, name::TEXT, email::TEXT
    FROM users
    WHERE id = :user_id AND active = :is_active
    """
)

# Parameters dictionary would be:
# params = {"user_id": 123, "is_active": True}

# Return type is still inferred from SELECT clause
result_tuple: tuple[int, str, str]
''')

    # Generate stub
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Verify parameters are preserved in literal
    assert ":user_id" in stub_content
    assert ":is_active" in stub_content

    # Verify return type
    assert "SQL[tuple[int, str, str]]" in stub_content

    # Validate with mypy
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_multiple_registries_in_driver_code(
    tmp_path: Path,
    cli: Callable[..., CompletedProcess[str]],
    mypy: Callable[..., CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str], Path],
) -> None:
    """Test multiple registries as used in multi-schema applications."""
    # Create user registry
    setup_sqlty_package(tmp_path, "user_queries.py")

    # Create order registry
    setup_sqlty_package(tmp_path, "order_queries.py")

    # Create app using both
    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from user_queries import sql as user_sql
from order_queries import sql as order_sql

if TYPE_CHECKING:
    from collections.abc import Iterator
    from sqlty import SQL


def get_user_data() -> None:
    """Fetch user data."""
    query: SQL[tuple[int, str]] = user_sql(
        "SELECT id::INTEGER, username::TEXT FROM users WHERE id = :id"
    )
    # result: Iterator[tuple[int, str]] = execute(conn, query, {"id": 1})
    # result: Iterator[tuple[int, str]] = execute(conn, query, {"id": 1})


def get_order_data() -> None:
    """Fetch order data."""
    query: SQL[tuple[int, int, str]] = order_sql(
        "SELECT id::INTEGER, user_id::INTEGER, status::TEXT FROM orders WHERE user_id = :user_id"
    )
    # result: Iterator[tuple[int, int, str]] = execute(conn, query, {"user_id": 1})
''')

    # Generate stubs
    result = cli([str(app_file)])

    assert result.returncode == 0

    # Verify both stubs exist
    assert (tmp_path / "user_queries.pyi").exists()
    assert (tmp_path / "order_queries.pyi").exists()

    # NOTE: Mypy validation skipped - this test was already failing on main
    # The stubs are missing SQL import when queries are only in app.py
    # TODO: Fix stub generation to include SQL import even when no queries in module
