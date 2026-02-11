"""End-to-end tests for multi-registry patterns.

Tests the workflow with multiple SQL registries in the same project.
"""

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
from inline_snapshot import snapshot

pytestmark = pytest.mark.skip(reason="Stub generation CLI (sqlty generate) deferred — see PLAN-FUTURE.md")


def test_multi_registry_different_files(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    mypy: Callable[..., subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str], Path],
) -> None:
    """Test multiple registries in different files."""
    # Create first registry
    setup_sqlty_package(tmp_path, "user_queries.py")

    # Create second registry
    setup_sqlty_package(tmp_path, "post_queries.py")

    # Create app using both registries
    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from user_queries import sql as user_sql
from post_queries import sql as post_sql

if TYPE_CHECKING:
    from sqlty import SQL

# User queries
users: SQL[tuple[int, str]] = user_sql("SELECT id::INTEGER, name::TEXT FROM users")

# Post queries
posts: SQL[tuple[int, str]] = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
""")

    # Create pyproject.toml to configure the functions with SQL wrapping
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "user_queries.sql"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "post_queries.sql"
query_param = "query"
""")

    # Generate stubs
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Verify both stub files were created
    user_stub = tmp_path / "user_queries.pyi"
    post_stub = tmp_path / "post_queries.pyi"

    assert user_stub.exists()
    assert post_stub.exists()

    # Snapshot user stub
    user_stub_content = user_stub.read_text()
    assert user_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
""")

    # Snapshot post stub
    post_stub_content = post_stub.read_text()
    assert post_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT id::INTEGER, title::TEXT FROM posts']) -> SQL[tuple[int, str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
""")

    # Validate with mypy
    mypy_result = mypy([str(app_file), "--strict"])

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_multi_registry_same_query_different_registries(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str], Path],
) -> None:
    """Test that same query in different registries creates separate overloads."""
    # Create first registry
    setup_sqlty_package(tmp_path, "registry1.py", "sql")

    # Create second registry
    setup_sqlty_package(tmp_path, "registry2.py", "sql")

    # Create app using the same query with both registries
    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from registry1 import sql as sql1
from registry2 import sql as sql2

if TYPE_CHECKING:
    from sqlty import SQL

# Same query used by both registries
query1: SQL[tuple[int]] = sql1("SELECT 1::INTEGER")
query2: SQL[tuple[int]] = sql2("SELECT 1::INTEGER")
""")

    # Generate stubs
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Both stubs should have the query
    stub1 = tmp_path / "registry1.pyi"
    stub2 = tmp_path / "registry2.pyi"

    assert stub1.exists()
    assert stub2.exists()

    stub1_content = stub1.read_text()
    stub2_content = stub2.read_text()

    # Both should contain the query
    assert "SELECT 1::INTEGER" in stub1_content
    assert "SELECT 1::INTEGER" in stub2_content


def test_nested_package_structure(
    tmp_path: Path, cli: Callable[..., subprocess.CompletedProcess[str]]
) -> None:
    """Test multi-registry in nested package structure."""
    # Create package structure
    package_dir = tmp_path / "myapp"
    package_dir.mkdir()
    (package_dir / "__init__.py").write_text("")

    queries_dir = package_dir / "queries"
    queries_dir.mkdir()
    (queries_dir / "__init__.py").write_text("")

    # Create registry in nested package
    users_file = queries_dir / "users.py"
    users_file.write_text("""from sqlty import sqlty


@sqlty
def sql(query: str):
    return query
""")

    # Create app that uses the nested registry
    app_file = package_dir / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from myapp.queries.users import sql

if TYPE_CHECKING:
    from sqlty import SQL

query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, username::TEXT FROM users")
""")

    # Create pyproject.toml at root
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "myapp.queries.users.sql"
query_param = "query"
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    print(result.stdout)
    assert result.returncode == 0

    # Verify stub was created
    stub_file = queries_dir / "users.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER, username::TEXT FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql(query: Literal['SELECT id::INTEGER, username::TEXT FROM users']) -> SQL[tuple[int, str]]:
    ...

@overload
def sql(query: str) -> str:
    ...
""")


def test_multi_registry_same_file_no_duplicate_aliases(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
    setup_sqlty_package: Callable[[Path, str, str | list[str]], Path],
) -> None:
    """Test that multiple registries in the same file don't create duplicate aliases."""
    # Create a single file with two registries using the fixture
    setup_sqlty_package(tmp_path, "queries.py", ["sql_a", "sql_b"])

    # Create app that uses both registries
    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql_a, sql_b

if TYPE_CHECKING:
    from sqlty import SQL

# Use sql_a
query_a: SQL[tuple[int]] = sql_a("SELECT 1::INTEGER")

# Use sql_b
query_b: SQL[tuple[str]] = sql_b("SELECT 'hello'::TEXT")
""")

    # Generate stubs
    result = cli([str(tmp_path)])

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Both functions should be present
    assert "@overload" in stub_content
    assert "def sql_a(" in stub_content
    assert "def sql_b(" in stub_content

    # No duplicate aliases should be present
    # Since both functions have different names, no aliases are needed
    alias_count = stub_content.count("sql = ")
    assert alias_count == 0, f"Expected 0 aliases but found {alias_count} in:\n{stub_content}"

    # Snapshot the result to verify format
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL


@overload
def sql_a(query: Literal['SELECT 1::INTEGER']) -> SQL[tuple[int]]:
    ...

@overload
def sql_a(query: str) -> str:
    ...

@overload
def sql_b(query: Literal["SELECT 'hello'::TEXT"]) -> SQL[tuple[str]]:
    ...

@overload
def sql_b(query: str) -> str:
    ...
""")


def test_multi_registry_separate_stubs_per_file(
    tmp_path: Path,
    cli: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """Test that multiple registries in different files generate separate stubs.

    Regression test for the bug where multiple files in the same directory
    that define functions with the same name (e.g., both user_queries.py and
    post_queries.py define sql()) would have their stubs incorrectly combined
    into a single .pyi file.

    EXPECTED: Each file should get its own .pyi file with only its own queries.
    This test uses registered functions (pyproject.toml) not @sqlty decorators.
    """
    # Create user_queries.py with registered function (no decorator needed)
    user_queries = tmp_path / "user_queries.py"
    user_queries.write_text("""\"\"\"User-related queries registry.

Functions are registered in pyproject.toml.
\"\"\"
from typing import Any


def sql(query: str) -> Any:
    \"\"\"Execute user queries.\"\"\"
    return query
""")

    # Create post_queries.py with registered function (no decorator needed)
    post_queries = tmp_path / "post_queries.py"
    post_queries.write_text("""\"\"\"Post-related queries registry.

Functions are registered in pyproject.toml.
\"\"\"
from typing import Any


def sql(query: str) -> Any:
    \"\"\"Execute post queries.\"\"\"
    return query
""")

    # Create app.py that uses both registries
    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from user_queries import sql as user_sql
from post_queries import sql as post_sql

if TYPE_CHECKING:
    from sqlty import SQL

# User queries
user_data: SQL[tuple[int, str]] = user_sql("SELECT id::INTEGER, name::TEXT FROM users")

# Post queries
post_data: SQL[tuple[int, str]] = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
""")

    # Create pyproject.toml with both registries
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text("""[tool.sqlty]
dialect = "postgres"

[[tool.sqlty.functions]]
import_path = "user_queries.sql"
query_param = "query"

[[tool.sqlty.functions]]
import_path = "post_queries.sql"
query_param = "query"
""")

    # Generate stubs for the entire directory
    result = cli([str(tmp_path)])
    assert result.returncode == 0, f"CLI failed: {result.stdout}\n{result.stderr}"

    # Verify both stub files were created
    user_stub = tmp_path / "user_queries.pyi"
    post_stub = tmp_path / "post_queries.pyi"

    assert user_stub.exists(), "user_queries.pyi should exist"
    assert post_stub.exists(), "post_queries.pyi should exist"

    # Verify each stub contains only its own queries
    user_stub_content = user_stub.read_text()
    post_stub_content = post_stub.read_text()

    # User stub should have the user query
    assert "FROM users" in user_stub_content, "user_queries.pyi should contain user query"
    assert "FROM posts" not in user_stub_content, "user_queries.pyi should not contain post queries"

    # Post stub should have the post query
    assert "FROM posts" in post_stub_content, "post_queries.pyi should contain post query"
    assert "FROM users" not in post_stub_content, "post_queries.pyi should not contain user queries"
