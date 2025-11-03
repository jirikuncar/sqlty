"""Test --mode flag for stub generation."""

from pathlib import Path
from subprocess import run


def test_mode_incremental(tmp_path: Path) -> None:
    """Test --mode=incremental flag."""
    source = """
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
q1 = sql("SELECT 1")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate initial stub
    result = run(
        ["python", "-m", "sqlty", str(source_file), "--mode=incremental"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    assert "Generated stub file" in result.stdout

    stub_file = source_file.with_suffix(".pyi")
    assert stub_file.exists()
    original_content = stub_file.read_text()

    # Add a new query and regenerate incrementally
    source_file.write_text(source + '\nq2 = sql("SELECT 2")\n')
    result = run(
        ["python", "-m", "sqlty", str(source_file), "--mode=incremental"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0

    new_content = stub_file.read_text()
    assert "SELECT 1" in new_content
    assert "SELECT 2" in new_content
    # Should have added, not replaced
    assert len(new_content) > len(original_content)


def test_mode_full(tmp_path: Path) -> None:
    """Test --mode=full flag."""
    source = """
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
q1 = sql("SELECT 1")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Generate stub with full mode
    result = run(
        ["python", "-m", "sqlty", str(source_file), "--mode=full"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    assert "Generated stub file" in result.stdout

    stub_file = source_file.with_suffix(".pyi")
    assert stub_file.exists()
    content = stub_file.read_text()
    assert "SELECT 1" in content


def test_mode_prune_placeholder(tmp_path: Path) -> None:
    """Test --mode=prune flag (placeholder until prune logic is implemented)."""
    source = """
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
q1 = sql("SELECT 1")
"""
    source_file = tmp_path / "test.py"
    source_file.write_text(source)

    # Prune mode should work but log that it's not yet fully implemented
    result = run(
        ["python", "-m", "sqlty", str(source_file), "--mode=prune"],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    # Should generate stub in full mode for now
    assert "Generated stub file" in result.stdout

    stub_file = source_file.with_suffix(".pyi")
    assert stub_file.exists()
