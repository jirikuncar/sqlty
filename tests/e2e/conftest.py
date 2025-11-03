"""Pytest fixtures for e2e tests."""

import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest


@pytest.fixture
def cli() -> Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]]:
    """Fixture to run sqlty CLI commands.

    Returns a callable that runs the sqlty CLI with the given arguments.

    Args:
        args: List of arguments to pass to sqlty (e.g., [str(tmp_path), "--mode=full"])
        kwargs: Optional keyword arguments to pass to subprocess.run

    Returns:
        A CompletedProcess instance from subprocess.run

    Example:
        result = cli([str(tmp_path), "--mode=full"])
        assert result.returncode == 0
    """

    def _run_cli(
        args: list[str | Path], kwargs: dict[str, Any] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run sqlty CLI with given arguments."""
        if kwargs is None:
            kwargs = {}

        # Set default kwargs for CLI execution
        default_kwargs = {
            "capture_output": True,
            "text": True,
        }
        default_kwargs.update(kwargs)

        # Convert Path objects to strings
        str_args = [str(arg) for arg in args]

        return subprocess.run(
            [sys.executable, "-m", "sqlty"] + str_args,
            **default_kwargs,
        )

    return _run_cli


@pytest.fixture
def setup_queries_file() -> Callable[[Path, str, str | None], Path]:
    """Fixture to create a queries.py file with SQLRegistry.

    Returns a callable that creates a queries.py file with the given
    registry class name and optional docstring.

    Args:
        tmp_path: The temporary directory path
        class_name: Name of the SQLRegistry class (default: "Query")
        docstring: Optional docstring for the class

    Returns:
        Path to the created queries.py file

    Example:
        queries_file = setup_queries_file(tmp_path, "Query")
        queries_file = setup_queries_file(tmp_path, "UserQuery", "User-related queries.")
    """

    def _create_queries_file(
        tmp_path: Path, class_name: str = "Query", docstring: str | None = None
    ) -> Path:
        """Create a queries.py file with SQLRegistry."""
        queries_file = tmp_path / "queries.py"
        
        docstring_line = f'    """{docstring}"""\n\n' if docstring else ""
        
        content = f'''from sqlty import SQLRegistry


class {class_name}(SQLRegistry):
{docstring_line}    pass


sql = {class_name}.sql
'''
        queries_file.write_text(content)
        return queries_file

    return _create_queries_file


@pytest.fixture
def setup_registry_file() -> Callable[[Path, str, str, str | None], Path]:
    """Fixture to create a custom registry file.

    Returns a callable that creates a registry file with a custom name,
    class name, and optional docstring.

    Args:
        tmp_path: The temporary directory path
        filename: Name of the file (e.g., "user_queries.py")
        class_name: Name of the SQLRegistry class
        docstring: Optional docstring for the class

    Returns:
        Path to the created registry file

    Example:
        user_file = setup_registry_file(tmp_path, "user_queries.py", "UserQuery", "User queries.")
    """

    def _create_registry_file(
        tmp_path: Path, filename: str, class_name: str, docstring: str | None = None
    ) -> Path:
        """Create a custom registry file."""
        registry_file = tmp_path / filename
        
        docstring_line = f'    """{docstring}"""\n\n' if docstring else ""
        
        content = f'''from sqlty import SQLRegistry


class {class_name}(SQLRegistry):
{docstring_line}    pass


sql = {class_name}.sql
'''
        registry_file.write_text(content)
        return registry_file

    return _create_registry_file


@pytest.fixture
def mypy() -> Callable[[list[str | Path], dict[str, Any] | None], subprocess.CompletedProcess[str]]:
    """Fixture to run mypy type checker.

    Returns a callable that runs mypy with the given arguments.

    Args:
        args: List of arguments to pass to mypy (e.g., [str(app_file), "--strict"])
        kwargs: Optional keyword arguments to pass to subprocess.run

    Returns:
        A CompletedProcess instance from subprocess.run

    Example:
        result = mypy([str(app_file), "--strict"])
        assert result.returncode == 0
    """

    def _run_mypy(
        args: list[str | Path], kwargs: dict[str, Any] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run mypy with given arguments."""
        if kwargs is None:
            kwargs = {}

        # Set default kwargs for mypy execution
        default_kwargs = {
            "capture_output": True,
            "text": True,
        }
        default_kwargs.update(kwargs)

        # Convert Path objects to strings
        str_args = [str(arg) for arg in args]

        return subprocess.run(
            [sys.executable, "-m", "mypy"] + str_args,
            **default_kwargs,
        )

    return _run_mypy
