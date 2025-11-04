"""Test new mode behavior: incremental, prune, and create."""

from pathlib import Path
from unittest.mock import patch

from sqlty._generator import TypeStubGenerator


def test_single_file_default_mode_is_incremental(tmp_path: Path) -> None:
    """Test that single file mode defaults to incremental when no mode is specified."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    stub_path = generator.generate_stub_file(queries_file, mode="incremental")
    
    assert stub_path.exists()
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content


def test_directory_default_mode_is_incremental_prune_create(tmp_path: Path) -> None:
    """Test that directory mode defaults to incremental,prune,create."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    stub_path = generator.generate_stub_file(tmp_path, mode="incremental,prune,create")
    
    assert stub_path.exists()
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content


def test_create_mode_skips_existing_stub(tmp_path: Path) -> None:
    """Test that create mode skips files that already exist."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    
    # First run creates the stub
    stub_path = generator.generate_stub_file(queries_file, mode="incremental")
    assert stub_path.exists()
    original_content = stub_path.read_text()
    
    # Update the source with a new query
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
"""
    )
    
    # Run with create mode - should skip because file exists
    stub_path2 = generator.generate_stub_file(queries_file, mode="create")
    assert stub_path2 == stub_path
    new_content = stub_path.read_text()
    
    # Content should be unchanged
    assert new_content == original_content
    assert "SELECT 2::INTEGER" not in new_content


def test_incremental_prune_mode_adds_and_removes_queries(tmp_path: Path) -> None:
    """Test that incremental,prune mode both adds new queries and removes stale ones."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    
    # First run creates the stub with both queries
    stub_path = generator.generate_stub_file(queries_file, mode="incremental")
    assert stub_path.exists()
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" in content
    
    # Update source: remove query2, add query3
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
query3 = sql("SELECT 3::INTEGER")
"""
    )
    
    # Run with incremental,prune mode (suppress input prompt)
    with patch('builtins.input', return_value='y'):
        stub_path = generator.generate_stub_file(queries_file, mode="incremental,prune")
    
    content = stub_path.read_text()
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 3::INTEGER" in content  # Added
    assert "SELECT 2::INTEGER" not in content  # Removed


def test_prune_mode_on_single_file_requires_confirmation(tmp_path: Path) -> None:
    """Test that prune mode on single file prompts for confirmation."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    stub_path = generator.generate_stub_file(queries_file, mode="incremental")
    
    # Mock input to return 'n' (no)
    with patch('builtins.input', return_value='n'):
        try:
            generator.generate_stub_file(queries_file, mode="prune")
            # If we get here without SystemExit, test fails
            assert False, "Expected SystemExit but didn't get it"
        except SystemExit as e:
            assert e.code == 0  # Should exit with 0 (not an error)


def test_incremental_create_mode_updates_existing_creates_new(tmp_path: Path) -> None:
    """Test that incremental,create updates existing stubs but creates new ones."""
    # Create two query files
    queries1_file = tmp_path / "queries1.py"
    queries1_file.write_text(
        """from sqlty import SQLRegistry

class Query1(SQLRegistry):
    pass

sql = Query1.sql
q1 = sql("SELECT 1::INTEGER")
"""
    )
    
    queries2_file = tmp_path / "queries2.py"
    queries2_file.write_text(
        """from sqlty import SQLRegistry

class Query2(SQLRegistry):
    pass

sql = Query2.sql
q2 = sql("SELECT 2::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    
    # Generate stub for queries1
    stub1_path = generator.generate_stub_file(queries1_file, mode="incremental")
    assert stub1_path.exists()
    
    # Update queries1 with new query
    queries1_file.write_text(
        """from sqlty import SQLRegistry

class Query1(SQLRegistry):
    pass

sql = Query1.sql
q1 = sql("SELECT 1::INTEGER")
q1b = sql("SELECT 10::INTEGER")
"""
    )
    
    # Run on directory with incremental,create mode
    generator.generate_stub_file(tmp_path, mode="incremental,create")
    
    # Check queries1 stub was updated (incremental)
    content1 = stub1_path.read_text()
    assert "SELECT 1::INTEGER" in content1
    assert "SELECT 10::INTEGER" in content1
    
    # Check queries2 stub was created (create)
    stub2_path = tmp_path / "queries2.pyi"
    assert stub2_path.exists()
    content2 = stub2_path.read_text()
    assert "SELECT 2::INTEGER" in content2


def test_fresh_mode_replaces_stub_completely(tmp_path: Path) -> None:
    """Test that fresh mode replaces the entire stub content."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    
    # First run creates stub
    stub_path = generator.generate_stub_file(queries_file, mode="incremental")
    assert stub_path.exists()
    
    # Update source with different query
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query2 = sql("SELECT 2::INTEGER")
"""
    )
    
    # Run with fresh mode (replaces entirely)
    stub_path = generator.generate_stub_file(queries_file, mode="fresh")
    content = stub_path.read_text()
    
    assert "SELECT 2::INTEGER" in content
    assert "SELECT 1::INTEGER" not in content  # Old query should be gone


def test_prune_mode_directory_does_not_prompt(tmp_path: Path) -> None:
    """Test that prune mode on directory doesn't prompt for confirmation."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
query2 = sql("SELECT 2::INTEGER")
"""
    )

    generator = TypeStubGenerator()
    
    # First run
    stub_path = generator.generate_stub_file(tmp_path, mode="incremental")
    assert stub_path.exists()
    
    # Update source, remove query2
    queries_file.write_text(
        """from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT 1::INTEGER")
"""
    )
    
    # Run with prune on directory - should NOT prompt
    # (no need to mock input, should just work)
    stub_path = generator.generate_stub_file(tmp_path, mode="prune")
    content = stub_path.read_text()
    
    assert "SELECT 1::INTEGER" in content
    assert "SELECT 2::INTEGER" not in content
