"""Test for multiple candidate registry files with same name in different directories."""

import tempfile
from pathlib import Path

from sqlty._generator import TypeStubGenerator


def test_multiple_registry_candidates_same_name():
    """Test that the correct registry file is selected when multiple candidates exist.

    This tests the bug fix where the code was checking the wrong variable in the loop,
    causing it to potentially select the wrong registry file when multiple directories
    have files with the same name.

    The fix ensures that when looking for registry files, we check ALL calls to find
    which ones reference each candidate file, not just the current call in the outer loop.
    """
    with tempfile.TemporaryDirectory() as tmpdir:
        root = Path(tmpdir)

        # Create two directories with the same registry file name
        dir1 = root / "module1"
        dir2 = root / "module2"
        dir1.mkdir()
        dir2.mkdir()

        # Create registry files with the same name in both directories
        registry1 = dir1 / "queries.py"
        registry2 = dir2 / "queries.py"

        registry1.write_text("""from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
""")

        registry2.write_text("""from sqlty import SQLRegistry

class AnotherQuery(SQLRegistry):
    pass

sql = AnotherQuery.sql
""")

        # Create a file in dir1 that uses the registry
        user1 = dir1 / "users.py"
        user1.write_text("""from .queries import Query

user_query = Query.sql("SELECT id::INTEGER, name::TEXT FROM users")
""")

        # Create a file in dir2 that uses the registry
        user2 = dir2 / "users.py"
        user2.write_text("""from .queries import AnotherQuery

user_query = AnotherQuery.sql("SELECT id::INTEGER, email::TEXT FROM customers")
""")

        # Generate stubs for the entire root directory
        generator = TypeStubGenerator()

        # This should generate two separate stub files, one for each directory
        generator.generate_stub_file(root, mode="full")

        # Check that both stub files were created in their respective directories
        stub1 = dir1 / "queries.pyi"
        stub2 = dir2 / "queries.pyi"

        assert stub1.exists(), "Stub for dir1/queries.py should exist"
        assert stub2.exists(), "Stub for dir2/queries.py should exist"

        # Verify the content is different (matching the different queries)
        content1 = stub1.read_text()
        content2 = stub2.read_text()

        # The stubs should contain the literal queries from their respective directories
        assert "name" in content1.lower(), "First stub should reference user query with name field"
        assert "email" in content2.lower(), (
            "Second stub should reference customer query with email field"
        )
        assert content1 != content2, "Stubs should have different content"
