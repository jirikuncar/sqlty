"""End-to-end tests for runtime auto-generation with SQLTY_AUTO_GENERATE=1.

Test scenarios:
1. Multiple registries in a single file
2. Parallel subprocess calls to sql()
3. Concurrent access with file locking
4. Incremental stub generation at runtime
"""

import subprocess
import time
from pathlib import Path
from typing import Any

import pytest


def test_runtime_autogen_single_registry(tmp_path: Path) -> None:
    """Test runtime auto-generation for a single registry."""
    # Create a test script that uses sql() at runtime
    test_script = tmp_path / "test_queries.py"
    test_script.write_text("""
import os
import sys

# Enable auto-generation
os.environ["SQLTY_AUTO_GENERATE"] = "1"

# Import after setting env var
from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    pass

sql = UserQueries.sql

# Make sql calls
query1 = sql("SELECT id::INTEGER, name::TEXT FROM users")
query2 = sql("SELECT email::TEXT FROM users WHERE id = :id")
query3 = sql("SELECT COUNT(*)::INTEGER FROM users")

print("Queries executed")
""")

    # Run the script
    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
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
    assert "class UserQueries(SQLRegistry):" in stub_content
    
    # Verify the types are correct
    assert "SQL[tuple[int, str]]" in stub_content  # query1
    assert "SQL[tuple[str]]" in stub_content  # query2
    assert "SQL[tuple[int]]" in stub_content  # query3


def test_runtime_autogen_multiple_registries(tmp_path: Path) -> None:
    """Test runtime auto-generation with multiple registries in a single file."""
    test_script = tmp_path / "multi_queries.py"
    test_script.write_text("""
import os
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class UserQueries(SQLRegistry):
    pass

class PostQueries(SQLRegistry):
    pass

user_sql = UserQueries.sql
post_sql = PostQueries.sql

# Make calls to different registries
user_query = user_sql("SELECT id::INTEGER FROM users")
post_query = post_sql("SELECT id::INTEGER, title::TEXT FROM posts")
user_count = user_sql("SELECT COUNT(*)::INTEGER FROM users")

print("Multi-registry queries executed")
""")

    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
    )
    
    assert result.returncode == 0, f"Script failed: {result.stderr}"
    
    stub_file = tmp_path / "multi_queries.pyi"
    assert stub_file.exists(), "Stub file was not generated"
    
    stub_content = stub_file.read_text()
    
    # Verify both registry classes are present
    assert "class UserQueries(SQLRegistry):" in stub_content
    assert "class PostQueries(SQLRegistry):" in stub_content
    
    # Verify queries for each registry
    assert "SELECT id::INTEGER FROM users" in stub_content
    assert "SELECT id::INTEGER, title::TEXT FROM posts" in stub_content
    assert "SELECT COUNT(*)::INTEGER FROM users" in stub_content
    
    # Verify exports
    assert "user_sql = UserQueries.sql" in stub_content
    assert "post_sql = PostQueries.sql" in stub_content


def test_runtime_autogen_incremental(tmp_path: Path) -> None:
    """Test that runtime auto-generation works incrementally."""
    test_script = tmp_path / "incremental.py"
    
    # First run: create initial stub with one query
    test_script.write_text("""
import os
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
query1 = sql("SELECT id::INTEGER FROM users")
print("First run")
""")
    
    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert result.returncode == 0
    
    stub_file = tmp_path / "incremental.pyi"
    assert stub_file.exists()
    first_content = stub_file.read_text()
    assert "SELECT id::INTEGER FROM users" in first_content
    
    # Second run: add another query
    test_script.write_text("""
import os
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
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
    )
    assert result.returncode == 0
    
    second_content = stub_file.read_text()
    
    # Both queries should be present
    assert "SELECT id::INTEGER FROM users" in second_content
    assert "SELECT name::TEXT FROM users" in second_content


def test_runtime_autogen_parallel_processes(tmp_path: Path) -> None:
    """Test concurrent stub generation from multiple processes."""
    # Create multiple test scripts that will run in parallel
    scripts = []
    for i in range(3):
        script = tmp_path / f"parallel_{i}.py"
        script.write_text(f"""
import os
import time
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class Query{i}(SQLRegistry):
    pass

sql = Query{i}.sql

# Add a small delay to ensure overlap
time.sleep(0.1)

query = sql("SELECT id::INTEGER, value_{i}::TEXT FROM table_{i}")
print("Process {i} done")
""")
        scripts.append(script)
    
    # Run all scripts in parallel
    processes = []
    for script in scripts:
        proc = subprocess.Popen(
            [sys.executable, str(script)],
            cwd=str(tmp_path),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
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
        assert f"class Query{i}(SQLRegistry):" in stub_content
        assert f"SELECT id::INTEGER, value_{i}::TEXT FROM table_{i}" in stub_content


def test_runtime_autogen_with_existing_stub(tmp_path: Path) -> None:
    """Test that runtime auto-generation preserves existing stub content."""
    stub_file = tmp_path / "existing.pyi"
    
    # Create an existing stub with custom content
    existing_stub = """from typing import Literal, overload
from sqlty import SQL, SQLRegistry


class Query(SQLRegistry):
    \"\"\"Existing query registry.\"\"\"

    @overload
    @classmethod
    def sql(cls, query: Literal['SELECT existing::INTEGER FROM table']) -> SQL[tuple[int]]:
        ...

    @overload
    @classmethod
    def sql[T](cls, query: T) -> T:
        ...
sql = Query.sql
"""
    stub_file.write_text(existing_stub)
    
    # Create a script that adds a new query
    test_script = tmp_path / "existing.py"
    test_script.write_text("""
import os
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
new_query = sql("SELECT new::TEXT FROM table")
print("New query added")
""")
    
    result = subprocess.run(
        [sys.executable, str(test_script)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
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
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql
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


def test_runtime_autogen_performance_overhead(tmp_path: Path) -> None:
    """Test that runtime auto-generation has minimal performance overhead."""
    # Create two scripts: one with auto-gen, one without
    script_with = tmp_path / "with_autogen.py"
    script_with.write_text("""
import os
import time
os.environ["SQLTY_AUTO_GENERATE"] = "1"

from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

start = time.perf_counter()
for i in range(100):
    query = sql(f"SELECT id::INTEGER FROM users WHERE id = {i}")
end = time.perf_counter()

print(f"Time: {end - start:.4f}")
""")
    
    script_without = tmp_path / "without_autogen.py"
    script_without.write_text("""
import time
# NO auto-gen
from sqlty import SQLRegistry

class Query(SQLRegistry):
    pass

sql = Query.sql

start = time.perf_counter()
for i in range(100):
    query = sql(f"SELECT id::INTEGER FROM users WHERE id = {i}")
end = time.perf_counter()

print(f"Time: {end - start:.4f}")
""")
    
    # Run both and compare times
    result_with = subprocess.run(
        [sys.executable, str(script_with)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
    )
    
    result_without = subprocess.run(
        [sys.executable, str(script_without)],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        timeout=10,
    )
    
    assert result_with.returncode == 0
    assert result_without.returncode == 0
    
    # Extract times
    time_with = float(result_with.stdout.strip().split()[-1])
    time_without = float(result_without.stdout.strip().split()[-1])
    
    # Overhead should be reasonable (less than 10x slowdown)
    # Note: This is a rough check; actual overhead should be much smaller
    overhead_ratio = time_with / time_without if time_without > 0 else 1
    assert overhead_ratio < 10, f"Performance overhead too high: {overhead_ratio}x"


# Add sys import for all tests that use subprocess
import sys
