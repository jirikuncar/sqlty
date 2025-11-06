"""End-to-end tests for basic sqlty workflow.

Test the complete workflow:
1. Define source files with repositories and queries
2. Run pyi generator (via subprocess)
3. Snapshot result
4. Run mypy on queries to validate type annotations
"""

import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

from inline_snapshot import snapshot


def test_basic_query_generation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path], Path],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test basic query generation and mypy validation."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''"""Application using SQL queries."""

from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query1: SQL[tuple[int]] = sql("SELECT id::INTEGER FROM users")
query2: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
query3: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM users")
''')

    result = cli([str(tmp_path)])
    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    assert stub_content == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM users']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = Query.sql
''')

    mypy_result = mypy([str(app_file), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy validation failed: {mypy_result.stdout}"


def test_typed_query_annotation(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
    mypy: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
) -> None:
    """Test that explicitly typed queries work with mypy."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

q: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    assert stub_file.exists()

    stub_content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in stub_content
    assert "SQL[tuple[int]]" in stub_content

    mypy_result = mypy([str(app_file), "--strict"])
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_incremental_stub_update(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that incremental mode preserves existing queries while adding new ones."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query1: SQL[tuple[int]] = sql("SELECT id::INTEGER FROM users")
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    initial_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in initial_stub

    app_file.write_text("""from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query1: SQL[tuple[int]] = sql("SELECT id::INTEGER FROM users")
query2: SQL[tuple[str]] = sql("SELECT name::TEXT FROM users WHERE id = :id")
""")

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    updated_stub = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in updated_stub
    assert "SELECT name::TEXT FROM users WHERE id = :id" in updated_stub

    assert updated_stub == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER FROM users']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT name::TEXT FROM users WHERE id = :id']) -> SQL[tuple[str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = Query.sql
''')


def test_multiline_query_formatting(
    tmp_path: Path,
    cli: Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]],
    setup_queries_file: Callable[[Path, str, str | None], Path],
) -> None:
    """Test that multiline queries are properly formatted in Literal types."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text('''from typing import TYPE_CHECKING

from queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

query: SQL[tuple[int, str, str]] = sql("""
    SELECT
        id::INTEGER,
        name::TEXT,
        email::TEXT
    FROM users
    WHERE active = true
""")
''')

    result = cli([str(tmp_path)])
    assert result.returncode == 0

    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert '"""' in stub_content
    assert "SELECT" in stub_content
    assert "FROM users" in stub_content

    assert stub_content == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal['\\n    SELECT\\n        id::INTEGER,\\n        name::TEXT,\\n        email::TEXT\\n    FROM users\\n    WHERE active = true\\n']) -> SQL[tuple[int, str, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = Query.sql
''')
