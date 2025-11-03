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
