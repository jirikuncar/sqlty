"""End-to-end tests for runtime auto-generation with SQLTY_AUTO_GENERATE=1.

Test scenarios:
1. Multiple registries in a single file
2. Parallel subprocess calls to sql()
3. Concurrent access with file locking
4. Incremental stub generation at runtime
"""

import os
import subprocess
import sys
from pathlib import Path


def test_runtime_autogen_single_registry(tmp_path: Path, create_pyproject_toml: callable) -> None:
    """Test runtime auto-generation for a single registry."""
    # Create a test script that uses sql() at runtime
    test_script = tmp_path / "test_queries.py"
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

# Make sql calls
query1 = sql("SELECT id::INTEGER, name::TEXT FROM users")
query2 = sql("SELECT email::TEXT FROM users WHERE id = :id")
query3 = sql("SELECT COUNT(*)::INTEGER FROM users")

print("Queries executed")
""")

    # Run the script with SQLTY_AUTO_GENERATE env var
    import os

    # Register the function defined in test_queries.py
    create_pyproject_toml(tmp_path, [("test_queries.sql", "query")])

    env = os.environ.copy()
    env["SQLTY_AUTO_GENERATE"] = "1"

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )

    assert result.returncode == 0, f"Script failed: {result.stderr}"
    assert "Queries executed" in result.stdout

    # Check that stub was generated
    stub_file = tmp_path / "test_queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Verify all three queries are in the stub
    assert "SELECT id::INTEGER, name::TEXT FROM users" in stub_content
    assert "SELECT email::TEXT FROM users WHERE id = :id" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content
    assert "@overload" in stub_content
    assert "def sql(" in stub_content

    # Verify the types are correct
    assert "SQL[tuple[int, str]]" in stub_content  # query1
    assert "SQL[tuple[str]]" in stub_content  # query2
    assert "SQL[tuple[int]]" in stub_content  # query3


def test_runtime_autogen_multiple_registries(
    tmp_path: Path, create_pyproject_toml: callable
) -> None:
    """Test runtime auto-generation with multiple registries in a single file."""
    test_script = tmp_path / "multi_queries.py"
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def user_sql[T](query: T) -> T:
    return query

@sqlty
def post_sql[T](query: T) -> T:
    return query

# Make calls to different registries
user_query = user_sql("SELECT id::INTEGER FROM users")
post_query = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
user_count = user_sql("SELECT COUNT(*)::INTEGER FROM users")

print("Multi-registry queries executed")
""")

    import os

    # Register both functions defined in multi_queries.py
    create_pyproject_toml(
        tmp_path, [("multi_queries.user_sql", "query"), ("multi_queries.post_sql", "query")]
    )

    env = os.environ.copy()
    env["SQLTY_AUTO_GENERATE"] = "1"

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )

    assert result.returncode == 0, f"Script failed: {result.stderr}"

    stub_file = tmp_path / "multi_queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"

    stub_content = stub_file.read_text()

    # Verify both decorated functions are present
    assert "@overload" in stub_content
    assert "def user_sql(" in stub_content
    assert "def post_sql(" in stub_content

    # Verify queries for each registry
    assert "SELECT id::INTEGER FROM users" in stub_content
    assert "SELECT id::INTEGER, title::TEXT FROM posts" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content


def test_runtime_autogen_incremental(tmp_path: Path, create_pyproject_toml: callable) -> None:
    """Test that runtime auto-generation works incrementally."""
    test_script = tmp_path / "incremental.py"

    import os

    env = os.environ.copy()
    env["SQLTY_AUTO_GENERATE"] = "1"

    # First run: create initial stub with one query
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

query1 = sql("SELECT id::INTEGER FROM users")
print("First run")
""")

    # Register the function defined in incremental.py
    create_pyproject_toml(tmp_path, [("incremental.sql", "query")])

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )
    assert result.returncode == 0

    stub_file = tmp_path / "incremental.pyi"
    assert stub_file.exists()
    first_content = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in first_content

    # Second run: add another query
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

query1 = sql("SELECT id::INTEGER FROM users")
query2 = sql("SELECT name::TEXT FROM users")
print("Second run")
""")

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )
    assert result.returncode == 0

    second_content = stub_file.read_text()

    # Both queries should be present
    assert "SELECT id::INTEGER FROM users" in second_content
    assert "SELECT name::TEXT FROM users" in second_content


def test_runtime_autogen_parallel_processes(
    tmp_path: Path, create_pyproject_toml: callable
) -> None:
    """Test concurrent stub generation from multiple processes."""
    # Create multiple test scripts that will run in parallel
    import os

    env = os.environ.copy()
    env["SQLTY_AUTO_GENERATE"] = "1"

    scripts = []
    for i in range(3):
        script = tmp_path / f"parallel_{i}.py"
        script.write_text(f"""
import time
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

# Add a small delay to ensure overlap
time.sleep(0.1)

query = sql("SELECT id::INTEGER, value_{i}::TEXT FROM table_{i}")
print("Process {i} done")
""")
        scripts.append(script)

    # Register functions for all parallel_* scripts
    function_configs = [(f"parallel_{i}.sql", "query") for i in range(3)]
    create_pyproject_toml(tmp_path, function_configs)

    # Run all scripts in parallel
    processes = []
    for script in scripts:
        proc = subprocess.Popen(
            [sys.executable, str(script)],
            cwd=str(tmp_path),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        processes.append(proc)

    # Wait for all to complete
    results = []
    for proc in processes:
        stdout, stderr = proc.communicate(timeout=10)
        results.append((proc.returncode, stdout, stderr))

    # Verify all succeeded
    for i, (returncode, stdout, stderr) in enumerate(results):
        assert returncode == 0, f"Process {i} failed: {stderr}"
        assert f"Process {i} done" in stdout

    # Verify all stubs were generated
    for i in range(3):
        stub_file = tmp_path / f"parallel_{i}.pyi"
        assert stub_file.exists(), f"Stub file {i} was not generated"

        stub_content = stub_file.read_text()
        assert "@overload" in stub_content
        assert "def sql(" in stub_content
        assert f"SELECT id::INTEGER, value_{i}::TEXT FROM table_{i}" in stub_content


def test_runtime_autogen_with_existing_stub(
    tmp_path: Path, create_pyproject_toml: callable
) -> None:
    """Test that runtime auto-generation preserves existing stub content."""
    stub_file = tmp_path / "existing.pyi"

    # Create an existing stub with custom content
    existing_stub = """from typing import Literal, overload
from sqlty import SQL

@overload
def sql(query: Literal['SELECT existing::INTEGER FROM table']) -> SQL[tuple[int]]:
    ...

@overload
def sql(query: str) -> str:
    ...
"""
    stub_file.write_text(existing_stub)

    # Create a script that adds a new query
    test_script = tmp_path / "existing.py"
    test_script.write_text("""
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

new_query = sql("SELECT new::TEXT FROM table")
print("New query added")
""")

    env = os.environ.copy()
    env["SQLTY_AUTO_GENERATE"] = "1"

    # Register the function defined in existing.py
    create_pyproject_toml(tmp_path, [("existing.sql", "query")])

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )

    assert result.returncode == 0

    # Check that both old and new queries are present
    final_content = stub_file.read_text()
    assert "SELECT existing::INTEGER FROM table" in final_content
    assert "SELECT new::TEXT FROM table" in final_content


def test_runtime_autogen_disabled_by_default(tmp_path: Path) -> None:
    """Test that runtime auto-generation is disabled by default."""
    test_script = tmp_path / "default.py"
    test_script.write_text("""
# Do NOT set SQLTY_AUTO_GENERATE
from sqlty import sqlty

@sqlty
def sql[T](query: T) -> T:
    return query

query = sql("SELECT id::INTEGER FROM users")
print("Query executed without auto-gen")
""")

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
    )

    assert result.returncode == 0
    assert "Query executed without auto-gen" in result.stdout

    # Stub should NOT be generated
    stub_file = tmp_path / "default.pyi"
    assert not stub_file.exists(), "Stub file should not be generated when auto-gen is disabled"
