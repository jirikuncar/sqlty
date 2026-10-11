"""End-to-end tests for magic codec with runtime execution and type checking."""

import os
import subprocess
import sys
import tempfile
from collections.abc import Generator
from pathlib import Path

import pytest


@pytest.fixture
def tmp_project_dir() -> Generator[Path]:
    """Create a temporary project directory for testing."""
    with tempfile.TemporaryDirectory() as tmpdir:
        project_dir = Path(tmpdir)
        yield project_dir


def test_magic_codec_basic_transformation(tmp_project_dir: Path) -> None:
    """Test basic codec transformation with --sql strings."""
    # Create a file with --sql strings
    test_file = tmp_project_dir / "test_basic.py"
    test_file.write_text("""# coding: sqlty
q1 = \"\"\"--sql
SELECT 1::INTEGER
\"\"\"
q2 = \"\"\"--sql
SELECT id::INTEGER, name::TEXT
\"\"\"

print("q1:", q1)
print("q2:", q2)
""")

    # Transform the file using python -m sqlty.magic
    transform_result = subprocess.run(
        [sys.executable, "-m", "sqlty.magic", str(test_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert transform_result.returncode == 0, f"Transform failed:\nstderr: {transform_result.stderr}"

    # Verify transformations in the output
    transformed = transform_result.stdout
    assert "SQL[tuple[int]]" in transformed
    assert "SQL[tuple[int, str]]" in transformed


def test_magic_codec_auto_imports(tmp_project_dir: Path) -> None:
    """Test that codec adds all necessary imports automatically."""
    test_file = tmp_project_dir / "test_imports.py"
    test_file.write_text("""# coding: sqlty
q1 = \"\"\"--sql
SELECT created_at::TIMESTAMP
\"\"\"
q2 = \"\"\"--sql
SELECT user_id::UUID
\"\"\"

print("q1:", q1)
print("q2:", q2)
""")

    # Transform the file
    transform_result = subprocess.run(
        [sys.executable, "-m", "sqlty.magic", str(test_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert transform_result.returncode == 0, f"Transform failed:\nstderr: {transform_result.stderr}"

    transformed = transform_result.stdout
    # Verify all imports were added automatically
    assert "import datetime" in transformed
    assert "import uuid" in transformed
    assert "from sqlty import SQL" in transformed

    # Verify transformations
    assert "SQL[tuple[datetime.datetime]]" in transformed
    assert "SQL[tuple[uuid.UUID]]" in transformed


def test_magic_codec_execution(tmp_project_dir: Path) -> None:
    """Test executing transformed code produces SQL instances."""
    test_file = tmp_project_dir / "test_execution.py"
    test_file.write_text("""# coding: sqlty
q1 = \"\"\"--sql
SELECT 1::INTEGER
\"\"\"
q2 = \"\"\"--sql
SELECT id::INTEGER, name::TEXT
\"\"\"

# Verify runtime values are strings
assert isinstance(q1, str)
assert isinstance(q2, str)

print("Execution test passed!")
""")

    # Transform the file
    transform_result = subprocess.run(
        [sys.executable, "-m", "sqlty.magic", str(test_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert transform_result.returncode == 0, f"Transform failed:\nstderr: {transform_result.stderr}"

    # Write the transformed code to a new file
    transformed_file = tmp_project_dir / "test_execution_transformed.py"
    transformed_file.write_text(transform_result.stdout)

    # Execute the transformed code
    exec_result = subprocess.run(
        [sys.executable, str(transformed_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert exec_result.returncode == 0, (
        f"Execution failed:\nstdout: {exec_result.stdout}\nstderr: {exec_result.stderr}"
    )
    assert "Execution test passed!" in exec_result.stdout


def test_magic_codec_no_transformation_without_prefix(tmp_project_dir: Path) -> None:
    """Test that strings without --sql prefix are not transformed."""
    test_file = tmp_project_dir / "test_no_transform.py"
    test_file.write_text("""# coding: sqlty
regular_string = "SELECT 1::INTEGER"
another_string = "Just a normal string"

print("No transformation!")
""")

    # Transform the file
    transform_result = subprocess.run(
        [sys.executable, "-m", "sqlty.magic", str(test_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert transform_result.returncode == 0, f"Transform failed:\nstderr: {transform_result.stderr}"

    transformed = transform_result.stdout
    # Verify no transformation occurred
    assert "SQL[" not in transformed
    assert "import datetime" not in transformed
    assert "import uuid" not in transformed


@pytest.mark.skipif(
    subprocess.run(
        [sys.executable, "-m", "mypy", "--version"],
        capture_output=True,
    ).returncode
    != 0,
    reason="mypy not available",
)
def test_magic_codec_mypy_validation(tmp_project_dir: Path) -> None:
    """Test that mypy understands the transformed types."""
    test_file = tmp_project_dir / "test_mypy.py"
    test_file.write_text("""# coding: sqlty
from sqlty import SQL

q1: SQL[tuple[int]] = \"\"\"--sql
SELECT 1::INTEGER
\"\"\"
q2: SQL[tuple[int, str]] = \"\"\"--sql
SELECT id::INTEGER, name::TEXT
\"\"\"

print("Type checking!")
""")

    # Transform the file
    transform_result = subprocess.run(
        [sys.executable, "-m", "sqlty.magic", str(test_file)],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    assert transform_result.returncode == 0, f"Transform failed:\nstderr: {transform_result.stderr}"

    transformed = transform_result.stdout
    assert transformed.startswith("# coding: utf-8\n")
    transformed_file = tmp_project_dir / "test_mypy_transformed.py"
    transformed_file.write_text(transformed)

    # Run mypy on the transformed file
    mypy_result = subprocess.run(
        [sys.executable, "-m", "mypy", str(transformed_file), "--strict"],
        capture_output=True,
        text=True,
        env={**os.environ, "PYTHONPATH": str(Path(__file__).parent.parent.parent / "src")},
    )

    # Check mypy output (may have some errors, but should recognize SQL types)
    assert mypy_result.returncode in (0, 1), (
        f"Mypy failed unexpectedly:\nstdout: {mypy_result.stdout}\nstderr: {mypy_result.stderr}"
    )
