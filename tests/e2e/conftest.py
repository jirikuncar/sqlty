"""Pytest fixtures for e2e tests."""

import os
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

# Automatically mark all tests in tests/e2e/ as e2e
pytestmark = pytest.mark.e2e


def _create_cli_runner(
    module_name: str, path_env_var: str = "PYTHONPATH"
) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Create a CLI runner function for a given module.

    Args:
        module_name: The module to run (e.g., 'sqlty')
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
