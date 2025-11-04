"""Test pre-commit workflow with non-changing file scenario.

This test verifies that the _write_stub_if_changed function correctly handles
the pre-commit use-case where files should only be written when content changes.
"""

import os
import time
from pathlib import Path

import pytest

from sqlty._generator import TypeStubGenerator


def test_pre_commit_workflow_non_changing_file(tmp_path: Path) -> None:
    """Test the complete pre-commit workflow with non-changing files.

    This test follows the workflow described in the issue:
    1. Create new query
    2. Run generator
    3. Check pyi has been generated and typing is correct
    4. Run generator again - should return status 0 and pyi file should not be modified
    5. Add new query to source file
    6. Run generator in incremental mode
    7. Check typing and timestamp of pyi
    """
    # Step 1: Create new query
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
query1 = sql("SELECT id::INTEGER FROM users")
"""
    )

    # Step 2: Run generator
    generator = TypeStubGenerator(sql_dialect="postgres")
    stub_path = generator.generate_stub_file(queries_file)

    # Step 3: Check pyi has been generated and typing is correct
    assert stub_path.exists()
    assert stub_path == queries_file.with_suffix(".pyi")

    stub_content = stub_path.read_text()
    assert "class Query(SQLRegistry):" in stub_content
    assert "SELECT id::INTEGER FROM users" in stub_content
    assert "SQL[tuple[int]]" in stub_content
    assert "@overload" in stub_content

    # Get the initial timestamp and content
    initial_mtime = os.path.getmtime(stub_path)
    initial_content = stub_content

    # Wait a bit to ensure timestamp would be different if file is rewritten
    time.sleep(0.01)

    # Step 4: Run generator again - should return status 0 and pyi file should not be modified
    stub_path2 = generator.generate_stub_file(queries_file)
    assert stub_path2 == stub_path

    # Verify file was NOT modified (same timestamp and content)
    second_mtime = os.path.getmtime(stub_path)
    second_content = stub_path.read_text()

    assert second_mtime == initial_mtime, "File timestamp should not change when content is identical"
    assert second_content == initial_content, "File content should remain identical"

    # Wait a bit to ensure timestamp would be different if file is rewritten
    time.sleep(0.01)

    # Step 5: Add new query to source file
    queries_file.write_text(
        """from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
"""
    )

    # Step 6: Run generator in incremental mode
    stub_path3 = generator.generate_stub_file(queries_file, mode="incremental")
    assert stub_path3 == stub_path

    # Step 7: Check typing and timestamp of pyi
    final_mtime = os.path.getmtime(stub_path)
    final_content = stub_path.read_text()

    # Verify file WAS modified (different timestamp and content)
    assert final_mtime > initial_mtime, "File timestamp should change when new content is added"
    assert final_content != initial_content, "File content should be updated with new query"

    # Verify the new query is present
    assert "SELECT name::TEXT FROM users" in final_content
    assert "SQL[tuple[str]]" in final_content

    # Verify the old query is still present
    assert "SELECT id::INTEGER FROM users" in final_content
    assert "SQL[tuple[int]]" in final_content


def test_write_stub_if_changed_returns_correct_status(tmp_path: Path) -> None:
    """Test that _write_stub_if_changed returns correct boolean status."""
    generator = TypeStubGenerator()
    stub_path = tmp_path / "test.pyi"
    content = "# Test content\n"

    # First write should return True (file was written)
    result = generator._write_stub_if_changed(stub_path, content)
    assert result is True
    assert stub_path.exists()
    assert stub_path.read_text() == content

    # Second write with same content should return False (file not changed)
    result = generator._write_stub_if_changed(stub_path, content)
    assert result is False

    # Write with different content should return True
    new_content = "# Different content\n"
    result = generator._write_stub_if_changed(stub_path, new_content)
    assert result is True
    assert stub_path.read_text() == new_content


def test_write_stub_if_changed_preserves_timestamp(tmp_path: Path) -> None:
    """Test that _write_stub_if_changed preserves file timestamp when content is unchanged."""
    generator = TypeStubGenerator()
    stub_path = tmp_path / "test.pyi"
    content = "# Test content\n"

    # Write initial content
    generator._write_stub_if_changed(stub_path, content)
    initial_mtime = os.path.getmtime(stub_path)

    # Wait a bit
    time.sleep(0.01)

    # Write same content
    generator._write_stub_if_changed(stub_path, content)
    second_mtime = os.path.getmtime(stub_path)

    # Timestamp should not change
    assert second_mtime == initial_mtime


def test_write_stub_if_changed_updates_timestamp_on_change(tmp_path: Path) -> None:
    """Test that _write_stub_if_changed updates file timestamp when content changes."""
    generator = TypeStubGenerator()
    stub_path = tmp_path / "test.pyi"

    # Write initial content
    generator._write_stub_if_changed(stub_path, "# Initial\n")
    initial_mtime = os.path.getmtime(stub_path)

    # Wait a bit
    time.sleep(0.01)

    # Write different content
    generator._write_stub_if_changed(stub_path, "# Changed\n")
    final_mtime = os.path.getmtime(stub_path)

    # Timestamp should be different
    assert final_mtime > initial_mtime


def test_write_stub_if_changed_handles_nonexistent_file(tmp_path: Path) -> None:
    """Test that _write_stub_if_changed handles nonexistent files correctly."""
    generator = TypeStubGenerator()
    stub_path = tmp_path / "nonexistent.pyi"
    content = "# New file\n"

    # Writing to nonexistent file should return True and create the file
    result = generator._write_stub_if_changed(stub_path, content)
    assert result is True
    assert stub_path.exists()
    assert stub_path.read_text() == content


def test_full_mode_respects_write_if_changed(tmp_path: Path) -> None:
    """Test that full mode also respects the write_if_changed behavior."""
    queries_file = tmp_path / "queries.py"
    queries_file.write_text(
        """from sqlty import SQLRegistry


class Query(SQLRegistry):
    pass


sql = Query.sql
query1 = sql("SELECT id::INTEGER FROM users")
"""
    )

    generator = TypeStubGenerator(sql_dialect="postgres")

    # First generation
    stub_path = generator.generate_stub_file(queries_file, mode="fresh")
    initial_mtime = os.path.getmtime(stub_path)
    initial_content = stub_path.read_text()

    time.sleep(0.01)

    # Second generation with full mode (same queries)
    generator.generate_stub_file(queries_file, mode="full")
    second_mtime = os.path.getmtime(stub_path)
    second_content = stub_path.read_text()

    # File should not be rewritten when content is the same
    assert second_mtime == initial_mtime
    assert second_content == initial_content
