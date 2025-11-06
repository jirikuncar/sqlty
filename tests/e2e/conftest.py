"""Pytest fixtures for e2e tests."""

import os
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest


def _create_cli_runner(
    module_name: str, path_env_var: str = "PYTHONPATH"
) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Create a CLI runner function for a given module.

    Args:
        module_name: The module to run (e.g., 'sqlty' or 'sqlty.registries')
        path_env_var: The environment variable to set for module path (default: 'PYTHONPATH')

    Returns:
        A callable that runs the CLI with the given arguments
    """
    # Get the source directory path (assuming tests are in tests/e2e/)
    src_dir = Path(__file__).parent.parent.parent / "src"

    def _run_cli(
        args: list[str | Path], kwargs: dict[str, Any] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run CLI with given arguments."""
        # Set default kwargs for CLI execution
        run_kwargs: dict[str, Any] = {
            "capture_output": True,
            "text": True,
        }

        # Merge provided kwargs
        if kwargs:
            run_kwargs.update(kwargs)

        # Convert Path objects to strings
        str_args = [str(arg) for arg in args]

        # Set path environment variable to include src directory
        env = run_kwargs.get("env", os.environ.copy())
        if not isinstance(env, dict):
            env = os.environ.copy()
        path_value = env.get(path_env_var, "")
        if path_value:
            env[path_env_var] = f"{src_dir}:{path_value}"
        else:
            env[path_env_var] = str(src_dir)

        # Set the env in run_kwargs
        run_kwargs["env"] = env

        return subprocess.run(
            [sys.executable, "-m", module_name] + str_args,
            **run_kwargs,
        )

    return _run_cli


@pytest.fixture
def cli() -> Callable[..., subprocess.CompletedProcess[str]]:
    """Fixture to run sqlty CLI commands.

    Returns a callable that runs the sqlty CLI with the given arguments.

    Args:
        args: List of arguments to pass to sqlty (e.g., [str(tmp_path)])
        kwargs: Optional keyword arguments to pass to subprocess.run

    Returns:
        A CompletedProcess instance from subprocess.run

    Example:
        result = cli([str(tmp_path)])
        assert result.returncode == 0
    """
    return _create_cli_runner("sqlty")


@pytest.fixture
def registries_cli() -> Callable[..., subprocess.CompletedProcess[str]]:
    """Fixture to run sqlty.registries CLI commands.

    Returns a callable that runs the sqlty.registries CLI with the given arguments.

    Args:
        args: List of arguments to pass to sqlty.registries (e.g., [str(tmp_path)])
        kwargs: Optional keyword arguments to pass to subprocess.run

    Returns:
        A CompletedProcess instance from subprocess.run

    Example:
        result = registries_cli([str(tmp_path)])
        assert result.returncode == 0
    """
    return _create_cli_runner("sqlty.registries")


@pytest.fixture
def setup_queries_file() -> Callable[..., Path]:
    """Fixture to create a queries.py file with @sqlty decorator.

    Args:
        tmp_path: The temporary directory path
        filename: Name of the file to create (e.g., "queries.py") (default: "queries.py")
        function_name: Name of the SQL function (default: "sql")

    Returns:
        Path to the created queries.py file

    Example:
        queries_file = setup_queries_file(tmp_path, "queries.py", "sql")
        queries_file = setup_queries_file(tmp_path, "user_queries.py", "user_sql")
        queries_file = setup_queries_file(tmp_path)  # creates queries.py with sql function
    """

    def _create_queries_file(
        tmp_path: Path, filename: str = "queries.py", function_name: str = "sql"
    ) -> Path:
        """Create a queries.py file with @sqlty decorator or SQLRegistry class."""
        queries_file = tmp_path / filename

        if function_name[0].isupper():
            # Create SQLRegistry class for backward compatibility
            content = f"""from sqlty import SQLRegistry


class {function_name}(SQLRegistry):
    pass
"""
        else:
            # Create @sqlty decorated function
            content = f"""from typing import Any
from sqlty import sqlty


@sqlty
def {function_name}(query: str) -> Any:
    return query
"""
        queries_file.write_text(content)
        return queries_file

    return _create_queries_file


@pytest.fixture
def mypy() -> Callable[..., subprocess.CompletedProcess[str]]:
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
    # Get the source directory path (assuming tests are in tests/e2e/)
    src_dir = Path(__file__).parent.parent.parent / "src"

    def _run_mypy(
        args: list[str | Path], kwargs: dict[str, Any] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run mypy with given arguments."""
        if kwargs is None:
            kwargs = {}

        # Set default kwargs for mypy execution
        run_kwargs: dict[str, Any] = {
            "capture_output": True,
            "text": True,
        }

        # Only update with valid subprocess.run parameters
        valid_keys = {
            "capture_output",
            "text",
            "input",
            "stdin",
            "stdout",
            "stderr",
            "shell",
            "cwd",
            "timeout",
            "check",
            "encoding",
            "errors",
            "universal_newlines",
            "bufsize",
            "executable",
            "preexec_fn",
            "close_fds",
            "pass_fds",
            "restore_signals",
            "start_new_session",
            "process_group",
            "group",
            "user",
            "umask",
            "pipesize",
            "env",
            "extra_groups",
        }

        for key, value in (kwargs or {}).items():
            if key in valid_keys:
                run_kwargs[key] = value

        # Convert Path objects to strings
        str_args = [str(arg) for arg in args]

        # Set MYPYPATH to include src directory so mypy can find sqlty module
        env = run_kwargs.get("env", os.environ.copy())
        if not isinstance(env, dict):
            env = os.environ.copy()
        mypypath = env.get("MYPYPATH", "")
        if mypypath:
            env["MYPYPATH"] = f"{src_dir}:{mypypath}"
        else:
            env["MYPYPATH"] = str(src_dir)

        # Set the env in run_kwargs
        run_kwargs["env"] = env

        return subprocess.run(
            [sys.executable, "-m", "mypy"] + str_args,
            **run_kwargs,
        )

    return _run_mypy


@pytest.fixture
def stubtest() -> Callable[..., subprocess.CompletedProcess[str]]:
    """Fixture to run stubtest for validating stub files.

    Returns a callable that runs stubtest with the given arguments.

    Args:
        args: List of arguments to pass to stubtest (e.g., ["queries", "--concise"])
        kwargs: Optional keyword arguments to pass to subprocess.run

    Returns:
        A CompletedProcess instance from subprocess.run

    Example:
        result = stubtest(["queries", "--concise"], {"env": {"PYTHONPATH": str(tmp_path)}})
        assert result.returncode == 0
    """
    import os

    # Get the source directory path (assuming tests are in tests/e2e/)
    src_dir = Path(__file__).parent.parent.parent / "src"
    allowlist = Path(__file__).parent.parent.parent / ".stubtest-allowlist"

    def _run_stubtest(
        args: list[str | Path], kwargs: dict[str, Any] | None = None
    ) -> subprocess.CompletedProcess[str]:
        """Run stubtest with given arguments."""
        if kwargs is None:
            kwargs = {}

        # Set default kwargs for stubtest execution
        run_kwargs: dict[str, Any] = {
            "capture_output": True,
            "text": True,
        }

        # Only update with valid subprocess.run parameters
        valid_keys = {
            "capture_output",
            "text",
            "input",
            "stdin",
            "stdout",
            "stderr",
            "shell",
            "cwd",
            "timeout",
            "check",
            "encoding",
            "errors",
            "universal_newlines",
            "bufsize",
            "executable",
            "preexec_fn",
            "close_fds",
            "pass_fds",
            "restore_signals",
            "start_new_session",
            "process_group",
            "group",
            "user",
            "umask",
            "pipesize",
            "env",
            "extra_groups",
        }

        for key, value in (kwargs or {}).items():
            if key in valid_keys:
                run_kwargs[key] = value

        # Convert Path objects to strings
        str_args = [str(arg) for arg in args]

        # Add allowlist and ignore-unused-allowlist if allowlist exists and not already in args
        if allowlist.exists() and "--allowlist" not in str_args:
            str_args.extend(["--allowlist", str(allowlist), "--ignore-unused-allowlist"])

        # Set PYTHONPATH to include src directory so stubtest can find sqlty module
        env = run_kwargs.get("env", os.environ.copy())
        if not isinstance(env, dict):
            env = os.environ.copy()
        pythonpath = env.get("PYTHONPATH", "")
        if pythonpath:
            env["PYTHONPATH"] = f"{src_dir}:{pythonpath}"
        else:
            env["PYTHONPATH"] = str(src_dir)

        # Set the env in run_kwargs
        run_kwargs["env"] = env

        return subprocess.run(
            [sys.executable, "-m", "mypy.stubtest"] + str_args,
            **run_kwargs,
        )

    return _run_stubtest
