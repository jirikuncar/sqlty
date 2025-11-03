"""End-to-end tests for multi-registry patterns.

Tests the workflow with multiple SQL registries in the same project.
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_multi_registry_different_files(tmp_path: Path) -> None:
    """Test multiple registries in different files."""
    # Create first registry
    user_queries_file = tmp_path / "user_queries.py"
    user_queries_file.write_text('''from sqlty import SQLRegistry


class UserQuery(SQLRegistry):
    """User-related queries."""

    pass


sql = UserQuery.sql
''')

    # Create second registry
    post_queries_file = tmp_path / "post_queries.py"
    post_queries_file.write_text('''from sqlty import SQLRegistry


class PostQuery(SQLRegistry):
    """Post-related queries."""

    pass


sql = PostQuery.sql
''')

    # Create app using both registries
    app_file = tmp_path / "app.py"
    app_file.write_text('''from user_queries import sql as user_sql
from post_queries import sql as post_sql

# User queries
users = user_sql("SELECT id::INTEGER, name::TEXT FROM users")

# Post queries
posts = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
''')

    # Generate stubs
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

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

from sqlty import SQL, SQLRegistry

class UserQuery(SQLRegistry):
    \"\"\"User-related queries with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER, name::TEXT FROM users"],
    ) -> SQL[tuple[int, str]]:
        # from app:5
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = UserQuery.sql
""")

    # Snapshot post stub
    post_stub_content = post_stub.read_text()
    assert post_stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class PostQuery(SQLRegistry):
    \"\"\"Post-related queries with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER, title::TEXT FROM posts"],
    ) -> SQL[tuple[int, str]]:
        # from app:8
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = PostQuery.sql
""")

    # Validate with mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_multi_registry_same_query_different_registries(tmp_path: Path) -> None:
    """Test that same query in different registries creates separate overloads."""
    # Create first registry
    registry1_file = tmp_path / "registry1.py"
    registry1_file.write_text('''from sqlty import SQLRegistry


class Registry1(SQLRegistry):
    pass


sql = Registry1.sql
''')

    # Create second registry
    registry2_file = tmp_path / "registry2.py"
    registry2_file.write_text('''from sqlty import SQLRegistry


class Registry2(SQLRegistry):
    pass


sql = Registry2.sql
''')

    # Create app using the same query with both registries
    app_file = tmp_path / "app.py"
    app_file.write_text('''from registry1 import sql as sql1
from registry2 import sql as sql2

# Same query used by both registries
query1 = sql1("SELECT 1::INTEGER")
query2 = sql2("SELECT 1::INTEGER")
''')

    # Generate stubs
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

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


def test_nested_package_structure(tmp_path: Path) -> None:
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
    users_file.write_text('''from sqlty import SQLRegistry


class UserQuery(SQLRegistry):
    pass


sql = UserQuery.sql
''')

    # Create app that uses the nested registry
    app_file = package_dir / "app.py"
    app_file.write_text('''from myapp.queries.users import sql

query = sql("SELECT id::INTEGER, username::TEXT FROM users")
''')

    # Generate stubs
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0

    # Verify stub was created
    stub_file = queries_dir / "users.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER, username::TEXT FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class UserQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER, username::TEXT FROM users"],
    ) -> SQL[tuple[int, str]]:
        # from app:3
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = UserQuery.sql
""")
