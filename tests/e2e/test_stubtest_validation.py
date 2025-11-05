"""End-to-end tests validating stubtest checks on generated stubs.

These tests ensure that the generated stubs are consistent with the runtime
implementation using mypy's stubtest tool.
"""

from pathlib import Path


def test_stubtest_validates_basic_stubs(tmp_path: Path, cli, stubtest, setup_queries_file) -> None:
    """Test that stubtest validates basic generated stubs."""
    setup_queries_file(tmp_path)

    # Create a simple app file with a query
    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

query = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Generate stub
    result = cli([str(tmp_path), "--mode=full"])
    assert result.returncode == 0

    # Run stubtest on the queries module
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_stubtest_detects_stub_inconsistencies(
    tmp_path: Path, cli, stubtest, setup_queries_file
) -> None:
    """Test that stubtest detects inconsistencies between stub and implementation."""
    setup_queries_file(tmp_path)

    # Generate stub
    result = cli([str(tmp_path), "--mode=full"])
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
        {"env": {"PYTHONPATH": str(tmp_path)}},
    )
    # This should fail because we added a non-existent method
    assert stubtest_result.returncode != 0
    assert "nonexistent_method" in stubtest_result.stdout


def test_stubtest_with_multiple_queries(
    tmp_path: Path, cli, stubtest, setup_queries_file
) -> None:
    """Test stubtest validation with multiple queries in a registry."""
    setup_queries_file(tmp_path)

    app_file = tmp_path / "app.py"
    app_file.write_text("""from queries import sql

# Multiple queries of different types
count_query = sql("SELECT COUNT(*)::INTEGER FROM users")
user_query = sql("SELECT id::INTEGER, name::TEXT FROM users LIMIT 1")
detailed_query = sql("SELECT id::INTEGER, name::TEXT, email::TEXT FROM users")
""")

    # Generate stub
    result = cli([str(tmp_path), "--mode=full"])
    assert result.returncode == 0

    # Verify stub contains all query types
    stub_file = tmp_path / "queries.pyi"
    stub_content = stub_file.read_text()

    assert "SQL[tuple[int]]" in stub_content
    assert "SQL[tuple[int, str]]" in stub_content
    assert "SQL[tuple[int, str, str]]" in stub_content

    # Run stubtest
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


def test_stubtest_with_custom_registry(
    tmp_path: Path, cli, stubtest, setup_registry_file
) -> None:
    """Test stubtest validation with a custom named registry."""
    setup_registry_file(tmp_path, "user_queries.py", "UserQueries", "User-related queries.")

    app_file = tmp_path / "app.py"
    app_file.write_text("""from user_queries import sql

get_user = sql("SELECT id::INTEGER, name::TEXT FROM users WHERE id = :id")
list_users = sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

    # Generate stub
    result = cli([str(tmp_path), "--mode=full"])
    assert result.returncode == 0

    # Verify stub was created
    stub_file = tmp_path / "user_queries.pyi"
    assert stub_file.exists()

    # Run stubtest
    stubtest_result = stubtest(
        ["user_queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}},
    )
    assert stubtest_result.returncode == 0, f"stubtest failed: {stubtest_result.stdout}"


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
    result = cli([str(tmp_path), "--mode=full"])
    assert result.returncode == 0

    # Run stubtest - should pass because allowlist covers these cases
    stubtest_result = stubtest(
        ["queries", "--concise"],
        {"env": {"PYTHONPATH": str(tmp_path)}},
    )
    assert (
        stubtest_result.returncode == 0
    ), f"stubtest should pass with allowlist: {stubtest_result.stdout}"
