"""End-to-end tests for basic sqlty workflow.

Test the complete workflow:
1. Define source files with repositories and queries
2. Run pyi generator (via subprocess)
3. Snapshot result
4. Run mypy on queries to validate type annotations
"""

import subprocess
import sys
from pathlib import Path

from inline_snapshot import snapshot


def test_basic_query_generation(tmp_path: Path) -> None:
    """Test basic query generation and mypy validation."""
    # Step 1: Create source file with repository and queries
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''"""Query registry."""

from sqlty import SQLRegistry


class Query(SQLRegistry):
    """Application query registry."""

    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using SQL queries."""

from queries import sql

# Simple query with type cast
query1 = sql("SELECT id::INTEGER FROM users")

# Multi-column query
query2 = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")

# Count query
query3 = sql("SELECT COUNT(*)::INTEGER FROM users")
''')

    # Step 2: Run pyi generator
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Step 3: Verify stub file was created and snapshot its content
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Snapshot the generated stub - this captures the expected output
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Application query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER FROM users"],
    ) -> SQL[tuple[int]]:
        # from app:6
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id"],
    ) -> SQL[tuple[int, str]]:
        # from app:9
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT COUNT(*)::INTEGER FROM users"],
    ) -> SQL[tuple[int]]:
        # from app:12
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")

    # Step 4: Run mypy to validate type annotations
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
        cwd=str(tmp_path),
    )

    # Mypy should pass or have expected errors (we'll snapshot those too)
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"


def test_typed_query_annotation(tmp_path: Path) -> None:
    """Test that explicitly typed queries work with mypy."""
    # Create source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Explicit type annotation
q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT 1::INTEGER"],
    ) -> SQL[tuple[int]]:
        # from app:9
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")

    # Validate with mypy
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(app_file), "--strict"],
        capture_output=True,
        text=True,
    )

    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_incremental_stub_update(tmp_path: Path) -> None:
    """Test that adding new queries updates the stub file."""
    # Create initial source files
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

# Initial query
query1 = sql("SELECT id::INTEGER FROM users")
''')

    # Generate initial stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    initial_stub = stub_file.read_text()

    assert "SELECT id::INTEGER FROM users" in initial_stub

    # Step 5 (optional): Add a new query
    app_file.write_text('''from queries import sql

# Initial query
query1 = sql("SELECT id::INTEGER FROM users")

# New query added
query2 = sql("SELECT name::TEXT FROM users WHERE id = :id")
''')

    # Step 6: Run generator again
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    # Verify stub was updated
    updated_stub = stub_file.read_text()

    assert "SELECT id::INTEGER FROM users" in updated_stub
    assert "SELECT name::TEXT FROM users WHERE id = :id" in updated_stub

    # Snapshot the updated result
    assert updated_stub == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT id::INTEGER FROM users"],
    ) -> SQL[tuple[int]]:
        # from app:4
        ...

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal["SELECT name::TEXT FROM users WHERE id = :id"],
    ) -> SQL[tuple[str]]:
        # from app:7
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")


def test_multiline_query_formatting(tmp_path: Path) -> None:
    """Test that multiline queries are properly formatted in stubs."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text('''from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
''')

    app_file = tmp_path / "app.py"
    app_file.write_text('''from queries import sql

# Multiline query
query = sql("""
    SELECT
        id::INTEGER,
        name::TEXT,
        email::TEXT
    FROM users
    WHERE active = true
""")
''')

    # Generate stub
    result = subprocess.run(
        [sys.executable, "-m", "sqlty", str(tmp_path), "--mode=full"],
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    # Verify multiline formatting is preserved
    assert '"""' in stub_content
    assert "SELECT" in stub_content
    assert "FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload

from sqlty import SQL, SQLRegistry

class Query(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(
        cls,
        query: Literal[
            \"\"\"
    SELECT
        id::INTEGER,
        name::TEXT,
        email::TEXT
    FROM users
    WHERE active = true
\"\"\"
        ],
    ) -> SQL[tuple[int, str, str]]:
        # from app:4
        ...

    @overload
    @classmethod
    def sql[T](
        cls,
        query: T,
    ) -> T: ...

# Type alias for the sql function
sql = Query.sql
""")
