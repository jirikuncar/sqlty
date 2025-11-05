"""End-to-end tests for relative imports of registry subclasses.

Tests comprehensive relative import scenarios:
- Nested packages importing from parents
- Parents importing from children
- Siblings importing from each other
- Deep nested package structures
"""

from pathlib import Path

from inline_snapshot import snapshot


def test_child_imports_from_parent_package(tmp_path: Path, cli, mypy) -> None:
    """Test that a child package can import registry from parent package."""
    # Create parent package with registry
    parent_pkg = tmp_path / "myapp"
    parent_pkg.mkdir()
    (parent_pkg / "__init__.py").write_text("")

    # Create registry in parent package
    parent_registry = parent_pkg / "queries.py"
    parent_registry.write_text("""from sqlty import SQLRegistry


class AppQuery(SQLRegistry):
    pass


sql = AppQuery.sql
""")

    # Create child package
    child_pkg = parent_pkg / "users"
    child_pkg.mkdir()
    (child_pkg / "__init__.py").write_text("")

    # Create module in child that imports from parent using relative import
    child_module = child_pkg / "handlers.py"
    child_module.write_text("""from typing import TYPE_CHECKING

from ..queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from parent package
user_query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was created for parent registry
    stub_file = parent_pkg / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class AppQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = AppQuery.sql
""")

    # Validate with mypy
    mypy_result = mypy([str(child_module), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_parent_imports_from_child_package(tmp_path: Path, cli, mypy) -> None:
    """Test that a parent module can import registry from child package."""
    # Create parent package
    parent_pkg = tmp_path / "myapp"
    parent_pkg.mkdir()
    (parent_pkg / "__init__.py").write_text("")

    # Create child package with registry
    child_pkg = parent_pkg / "queries"
    child_pkg.mkdir()
    (child_pkg / "__init__.py").write_text("")

    child_registry = child_pkg / "user_queries.py"
    child_registry.write_text("""from sqlty import SQLRegistry


class UserQuery(SQLRegistry):
    pass


sql = UserQuery.sql
""")

    # Create module in parent that imports from child
    parent_module = parent_pkg / "app.py"
    parent_module.write_text("""from typing import TYPE_CHECKING

from myapp.queries.user_queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from child package
query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, username::TEXT FROM users")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was created
    stub_file = child_pkg / "user_queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

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
    def sql(cls, query: Literal['SELECT id::INTEGER, username::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = UserQuery.sql
""")

    # Validate with mypy
    mypy_result = mypy([str(parent_module), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_sibling_packages_import_from_each_other(tmp_path: Path, cli, mypy) -> None:
    """Test that sibling packages can import registries from each other."""
    # Create parent package
    parent_pkg = tmp_path / "myapp"
    parent_pkg.mkdir()
    (parent_pkg / "__init__.py").write_text("")

    # Create first sibling package with registry
    users_pkg = parent_pkg / "users"
    users_pkg.mkdir()
    (users_pkg / "__init__.py").write_text("")

    users_registry = users_pkg / "queries.py"
    users_registry.write_text("""from sqlty import SQLRegistry


class UserQuery(SQLRegistry):
    pass


sql = UserQuery.sql
""")

    # Create second sibling package with registry
    posts_pkg = parent_pkg / "posts"
    posts_pkg.mkdir()
    (posts_pkg / "__init__.py").write_text("")

    posts_registry = posts_pkg / "queries.py"
    posts_registry.write_text("""from sqlty import SQLRegistry


class PostQuery(SQLRegistry):
    pass


sql = PostQuery.sql
""")

    # Create module in second sibling that imports from first sibling
    posts_handler = posts_pkg / "handlers.py"
    posts_handler.write_text("""from typing import TYPE_CHECKING

from ..users.queries import sql as user_sql
from .queries import sql as post_sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use user registry from sibling package
users: SQL[tuple[int, str]] = user_sql("SELECT id::INTEGER, name::TEXT FROM users")

# Use post registry from same package
posts: SQL[tuple[int, str]] = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify both stub files were created
    users_stub = users_pkg / "queries.pyi"
    posts_stub = posts_pkg / "queries.pyi"

    assert users_stub.exists(), "Users stub file was not generated"
    assert posts_stub.exists(), "Posts stub file was not generated"

    users_stub_content = users_stub.read_text()
    assert "SELECT id::INTEGER, name::TEXT FROM users" in users_stub_content

    posts_stub_content = posts_stub.read_text()
    assert "SELECT id::INTEGER, title::TEXT FROM posts" in posts_stub_content

    # Snapshot users stub
    assert users_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class UserQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = UserQuery.sql
""")

    # Snapshot posts stub
    assert posts_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class PostQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, title::TEXT FROM posts']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = PostQuery.sql
""")

    # Validate with mypy
    mypy_result = mypy([str(posts_handler), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_deeply_nested_package_structure(tmp_path: Path, cli, mypy) -> None:
    """Test registry imports in deeply nested package structure."""
    # Create deep package structure: myapp/features/auth/queries/
    app_pkg = tmp_path / "myapp"
    app_pkg.mkdir()
    (app_pkg / "__init__.py").write_text("")

    features_pkg = app_pkg / "features"
    features_pkg.mkdir()
    (features_pkg / "__init__.py").write_text("")

    auth_pkg = features_pkg / "auth"
    auth_pkg.mkdir()
    (auth_pkg / "__init__.py").write_text("")

    queries_pkg = auth_pkg / "queries"
    queries_pkg.mkdir()
    (queries_pkg / "__init__.py").write_text("")

    # Create registry in deeply nested package
    auth_registry = queries_pkg / "user_auth.py"
    auth_registry.write_text("""from sqlty import SQLRegistry


class AuthQuery(SQLRegistry):
    pass


sql = AuthQuery.sql
""")

    # Create module that imports from deeply nested package
    auth_handler = auth_pkg / "handlers.py"
    auth_handler.write_text("""from typing import TYPE_CHECKING

from .queries.user_auth import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from nested package
query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, email::TEXT FROM users WHERE active = true")
""")

    # Also create a module at app level that imports from deep nested package
    app_module = app_pkg / "app.py"
    app_module.write_text("""from typing import TYPE_CHECKING

from myapp.features.auth.queries.user_auth import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from deeply nested package
query: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM users")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was created
    stub_file = queries_pkg / "user_auth.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    assert "SELECT id::INTEGER, email::TEXT FROM users WHERE active = true" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class AuthQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM users']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, email::TEXT FROM users WHERE active = true']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = AuthQuery.sql
""")

    # Validate both files with mypy
    mypy_result_handler = mypy([str(auth_handler), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result_handler.returncode == 0, (
        f"Mypy failed on handler: {mypy_result_handler.stdout}"
    )

    mypy_result_app = mypy([str(app_module), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result_app.returncode == 0, f"Mypy failed on app: {mypy_result_app.stdout}"


def test_relative_import_from_package_init(tmp_path: Path, cli, mypy) -> None:
    """Test that relative imports work from package __init__.py files."""
    # Create package structure
    app_pkg = tmp_path / "myapp"
    app_pkg.mkdir()

    # Create registry in subpackage
    queries_pkg = app_pkg / "queries"
    queries_pkg.mkdir()
    (queries_pkg / "__init__.py").write_text("")

    registry_file = queries_pkg / "db.py"
    registry_file.write_text("""from sqlty import SQLRegistry


class DBQuery(SQLRegistry):
    pass


sql = DBQuery.sql
""")

    # Create package __init__.py that imports registry using relative import
    app_init = app_pkg / "__init__.py"
    app_init.write_text("""from typing import TYPE_CHECKING

from .queries.db import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry imported in __init__.py
default_query: SQL[tuple[int]] = sql("SELECT 1::INTEGER")
""")

    # Create app module that uses the registry
    app_module = tmp_path / "main.py"
    app_module.write_text("""from typing import TYPE_CHECKING

from myapp.queries.db import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use the same registry
query: SQL[tuple[str]] = sql("SELECT 'hello'::TEXT")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was created
    stub_file = queries_pkg / "db.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()
    assert "SELECT 1::INTEGER" in stub_content
    assert "SELECT 'hello'::TEXT" in stub_content

    # Snapshot the result
    assert stub_content == snapshot('''\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class DBQuery(SQLRegistry):
    """Application-specific SQL query registry with type-safe overloads."""

    @overload
    @classmethod
    def sql(cls, query: Literal["SELECT 'hello'::TEXT"]) -> SQL[tuple[str]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT 1::INTEGER']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = DBQuery.sql
''')

    # Validate with mypy
    mypy_result = mypy([str(app_module), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_cross_package_relative_imports_with_aliases(tmp_path: Path, cli, mypy) -> None:
    """Test relative imports with aliases across different packages."""
    # Create package structure
    app_pkg = tmp_path / "myapp"
    app_pkg.mkdir()
    (app_pkg / "__init__.py").write_text("")

    # Create first registry
    db_pkg = app_pkg / "database"
    db_pkg.mkdir()
    (db_pkg / "__init__.py").write_text("")

    db_registry = db_pkg / "queries.py"
    db_registry.write_text("""from sqlty import SQLRegistry


class DatabaseQuery(SQLRegistry):
    pass


sql = DatabaseQuery.sql
""")

    # Create second registry
    api_pkg = app_pkg / "api"
    api_pkg.mkdir()
    (api_pkg / "__init__.py").write_text("")

    api_registry = api_pkg / "queries.py"
    api_registry.write_text("""from sqlty import SQLRegistry


class APIQuery(SQLRegistry):
    pass


sql = APIQuery.sql
""")

    # Create module that imports both with relative imports and aliases
    service_module = api_pkg / "service.py"
    service_module.write_text("""from typing import TYPE_CHECKING

from ..database.queries import sql as db_sql
from .queries import sql as api_sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use both registries with aliases
db_query: SQL[tuple[int, str]] = db_sql("SELECT id::INTEGER, name::TEXT FROM users")
api_query: SQL[tuple[str]] = api_sql("SELECT endpoint::TEXT FROM api_routes")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify both stub files were created
    db_stub = db_pkg / "queries.pyi"
    api_stub = api_pkg / "queries.pyi"

    assert db_stub.exists(), "Database stub file was not generated"
    assert api_stub.exists(), "API stub file was not generated"

    db_stub_content = db_stub.read_text()
    api_stub_content = api_stub.read_text()

    assert "SELECT id::INTEGER, name::TEXT FROM users" in db_stub_content
    assert "SELECT endpoint::TEXT FROM api_routes" in api_stub_content

    # Snapshot database stub
    assert db_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class DatabaseQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, name::TEXT FROM users']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = DatabaseQuery.sql
""")

    # Snapshot API stub
    assert api_stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class APIQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT endpoint::TEXT FROM api_routes']) -> SQL[tuple[str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = APIQuery.sql
""")

    # Validate with mypy
    mypy_result = mypy([str(service_module), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result.returncode == 0, f"Mypy failed: {mypy_result.stdout}"


def test_very_deep_nesting_with_level_3_plus_imports(tmp_path: Path, cli, mypy) -> None:
    """Test that relative imports with 3+ levels (e.g., from .... import) work correctly."""
    # Create a very deep package structure
    # myapp/features/auth/handlers/user/
    app_pkg = tmp_path / "myapp"
    app_pkg.mkdir()
    (app_pkg / "__init__.py").write_text("")

    # Create registry at app level
    app_registry = app_pkg / "queries.py"
    app_registry.write_text("""from sqlty import SQLRegistry


class AppQuery(SQLRegistry):
    pass


sql = AppQuery.sql
""")

    # Create deep nested structure
    features_pkg = app_pkg / "features"
    features_pkg.mkdir()
    (features_pkg / "__init__.py").write_text("")

    auth_pkg = features_pkg / "auth"
    auth_pkg.mkdir()
    (auth_pkg / "__init__.py").write_text("")

    handlers_pkg = auth_pkg / "handlers"
    handlers_pkg.mkdir()
    (handlers_pkg / "__init__.py").write_text("")

    # Create module at depth 4 that uses level 4 import (....queries)
    deep_handler = handlers_pkg / "user_handler.py"
    deep_handler.write_text("""from typing import TYPE_CHECKING

from ....queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from 3 levels up via .... import
query: SQL[tuple[int, str]] = sql("SELECT id::INTEGER, username::TEXT FROM users WHERE active = true")
""")

    # Also test level 3 import from one level less deep
    mid_handler = auth_pkg / "auth_handler.py"
    mid_handler.write_text("""from typing import TYPE_CHECKING

from ...queries import sql

if TYPE_CHECKING:
    from sqlty import SQL

# Use registry from 2 levels up via ... import
query: SQL[tuple[int]] = sql("SELECT COUNT(*)::INTEGER FROM sessions")
""")

    # Generate stubs
    result = cli([str(tmp_path)], {"cwd": str(tmp_path)})

    assert result.returncode == 0, f"Generator failed: {result.stderr}"

    # Verify stub was created
    stub_file = app_pkg / "queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Both queries should be in the stub
    assert "SELECT id::INTEGER, username::TEXT FROM users WHERE active = true" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM sessions" in stub_content

    # Snapshot the result
    assert stub_content == snapshot("""\
from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class AppQuery(SQLRegistry):
    \"\"\"Application-specific SQL query registry with type-safe overloads.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT COUNT(*)::INTEGER FROM sessions']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT id::INTEGER, username::TEXT FROM users WHERE active = true']) -> SQL[tuple[int, str]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = AppQuery.sql
""")

    # Validate with mypy
    mypy_result_deep = mypy([str(deep_handler), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result_deep.returncode == 0, (
        f"Mypy failed on deep handler: {mypy_result_deep.stdout}"
    )

    mypy_result_mid = mypy([str(mid_handler), "--strict"], {"cwd": str(tmp_path)})
    assert mypy_result_mid.returncode == 0, f"Mypy failed on mid handler: {mypy_result_mid.stdout}"
