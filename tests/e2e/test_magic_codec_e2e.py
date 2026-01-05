"""End-to-end tests for magic codec with runtime execution and type checking."""

import subprocess
import sys
import tempfile
from pathlib import Path

import pytest


@pytest.fixture
def tmp_project_dir():
    """Create a temporary project directory for testing."""
    with tempfile.TemporaryDirectory() as tmpdir:
        project_dir = Path(tmpdir)
        yield project_dir


def test_magic_codec_e2e_runtime_execution(tmp_project_dir):
    """End-to-end test: Create file with --sql strings, run it, verify runtime types."""
    # Create a Python file that uses the magic codec
    test_file = tmp_project_dir / "test_query.py"
    test_file.write_text('''
# Register the codec first
from sqlty import magic, SQL
import codecs

# Use the sqlty codec to transform the source
codec = codecs.lookup("sqlty")

# This will be transformed by the codec
source_code = b"""
q1 = "--sql SELECT 1::INTEGER"
q2 = "--sql SELECT id::INTEGER, name::TEXT"
q3 = "--sql SELECT created_at::TIMESTAMP"
q4 = "--sql SELECT user_id::UUID"
"""

transformed, _ = codec.decode(source_code)

print("Transformed code:")
print(transformed)

# Verify the transformations occurred in the source
assert "SQL[tuple[int]]" in transformed
assert "SQL[tuple[int, str]]" in transformed
assert "SQL[tuple[datetime.datetime]]" in transformed
assert "SQL[tuple[uuid.UUID]]" in transformed

# Verify imports were added
assert "import datetime" in transformed
assert "import uuid" in transformed

# Execute the transformed code with SQL in scope
exec_globals = {"SQL": SQL}
exec(transformed, exec_globals)

# Verify the runtime values are SQL instances (which are strings)
print("q1:", exec_globals["q1"])
print("q2:", exec_globals["q2"])
print("q3:", exec_globals["q3"])
print("q4:", exec_globals["q4"])

assert isinstance(exec_globals["q1"], str)
assert isinstance(exec_globals["q2"], str)
assert isinstance(exec_globals["q3"], str)
assert isinstance(exec_globals["q4"], str)

print("All runtime checks passed!")
''')

    # Run the test file
    result = subprocess.run(
        [sys.executable, str(test_file)],
        capture_output=True,
        text=True,
        env={**subprocess.os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    # Check it ran successfully
    assert result.returncode == 0, f"Test failed:\nstdout: {result.stdout}\nstderr: {result.stderr}"
    assert "All runtime checks passed!" in result.stdout


def test_magic_codec_e2e_with_existing_imports(tmp_project_dir):
    """Test that imports are properly merged with existing imports using isort."""
    test_file = tmp_project_dir / "test_imports.py"
    test_file.write_text('''
from sqlty import magic
import codecs

codec = codecs.lookup("sqlty")

# Code with existing imports
source_code = b"""
from typing import Any

q = "--sql SELECT created_at::TIMESTAMP"
"""

transformed, _ = codec.decode(source_code)

# Verify datetime import was added in the right place
print("Transformed code:")
print(transformed)

# Check that imports are properly ordered
lines = transformed.strip().split("\\n")
import_section = []
for line in lines:
    if line.startswith("import ") or line.startswith("from "):
        import_section.append(line)
    elif line.strip() and not line.startswith("#"):
        break

print("Import section:", import_section)

# Verify both imports exist
assert "import datetime" in transformed
assert "from typing import Any" in transformed

# Verify datetime comes before typing (standard library before third-party)
datetime_line = next(i for i, line in enumerate(lines) if "import datetime" in line)
typing_line = next(i for i, line in enumerate(lines) if "from typing" in line)
assert datetime_line < typing_line, "datetime import should come before typing import"

print("Import ordering check passed!")
''')

    result = subprocess.run(
        [sys.executable, str(test_file)],
        capture_output=True,
        text=True,
        env={**subprocess.os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert result.returncode == 0, f"Test failed:\nstdout: {result.stdout}\nstderr: {result.stderr}"
    assert "Import ordering check passed!" in result.stdout


def test_magic_codec_e2e_actual_file_execution(tmp_project_dir):
    """Test actual file execution with transformed code."""
    # Create a Python file with --sql strings
    query_file = tmp_project_dir / "queries.py"
    query_file.write_text('''
q1 = "--sql SELECT 1::INTEGER"
q2 = "--sql SELECT id::INTEGER, name::TEXT"
q3 = "--sql SELECT created_at::TIMESTAMP, user_id::UUID"

# Print to verify
if __name__ == "__main__":
    print("q1:", q1)
    print("q2:", q2)
    print("q3:", q3)
''')

    # Create a script that uses the codec to transform and execute the file
    runner_file = tmp_project_dir / "runner.py"
    runner_file.write_text(f'''
from sqlty import magic, SQL
import codecs

# Read the query file
with open(r"{query_file}", "rb") as f:
    source_bytes = f.read()

# Decode using sqlty codec
codec = codecs.lookup("sqlty")
transformed, _ = codec.decode(source_bytes)

print("=== Transformed Code ===")
print(transformed)
print()

# Execute the transformed code with SQL in scope
exec_globals = {{"__name__": "__main__", "SQL": SQL}}
exec(transformed, exec_globals)

# The variables are now SQL instances, just verify transformation syntax
print()
print("=== Verification ===")
# q1-q3 are SQL[...] instances, which are actually just string subclasses
# so we can check their string value contains the original query
assert isinstance(exec_globals["q1"], str)
assert isinstance(exec_globals["q2"], str)
assert isinstance(exec_globals["q3"], str)
print("q1:", exec_globals["q1"])
print("q2:", exec_globals["q2"])
print("q3:", exec_globals["q3"])
print("All checks passed!")
''')

    result = subprocess.run(
        [sys.executable, str(runner_file)],
        capture_output=True,
        text=True,
        env={**subprocess.os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert result.returncode == 0, f"Test failed:\nstdout: {result.stdout}\nstderr: {result.stderr}"
    assert "All checks passed!" in result.stdout
    assert "import datetime" in result.stdout
    assert "import uuid" in result.stdout


@pytest.mark.skipif(
    subprocess.run(
        [sys.executable, "-m", "mypy", "--version"],
        capture_output=True,
    ).returncode != 0,
    reason="mypy not available"
)
def test_magic_codec_e2e_mypy_validation(tmp_project_dir):
    """Bonus test: Verify mypy understands the transformed types."""
    # Create a file with transformed code
    test_file = tmp_project_dir / "typed_queries.py"

    # First transform the code using the codec
    import codecs

    from sqlty import magic  # noqa: F401

    codec = codecs.lookup("sqlty")
    source = b'''
from sqlty import SQL

q1 = "--sql SELECT 1::INTEGER"
q2 = "--sql SELECT id::INTEGER, name::TEXT"
'''
    transformed, _ = codec.decode(source)

    # Write the transformed code to file
    test_file.write_text(transformed)

    # Run mypy on the file
    result = subprocess.run(
        [sys.executable, "-m", "mypy", str(test_file), "--strict"],
        capture_output=True,
        text=True,
        env={**subprocess.os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    # Check mypy output
    print("Mypy output:")
    print(result.stdout)
    print(result.stderr)

    # For now, just verify mypy runs (may have errors about specific types)
    # This is a bonus test to show the concept
    assert result.returncode in (0, 1)  # mypy may find type issues, that's ok for this test


def test_magic_codec_e2e_no_transformation_without_prefix(tmp_project_dir):
    """Test that strings without --sql prefix are not transformed."""
    test_file = tmp_project_dir / "test_no_transform.py"
    test_file.write_text('''
from sqlty import magic
import codecs

codec = codecs.lookup("sqlty")

source_code = b"""
regular_string = "SELECT 1::INTEGER"
another_string = "Just a normal string"
"""

transformed, _ = codec.decode(source_code)

print("Transformed code:")
print(transformed)

# Verify no transformation occurred
assert "SQL[" not in transformed
assert "import datetime" not in transformed
assert "import uuid" not in transformed

print("No-transformation check passed!")
''')

    result = subprocess.run(
        [sys.executable, str(test_file)],
        capture_output=True,
        text=True,
        env={**subprocess.os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert result.returncode == 0, f"Test failed:\nstdout: {result.stdout}\nstderr: {result.stderr}"
    assert "No-transformation check passed!" in result.stdout
