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
        """Create a queries.py file with @sqlty decorator."""
        queries_file = tmp_path / filename

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
def setup_sqlty_package() -> Callable[..., Path]:
    """Fixture to create a complete sqlty package with pyproject.toml configuration.

    Creates a proper package structure with:
    - A Python file with @sqlty decorated function(s)
    - A pyproject.toml that registers the function(s)
    - Optionally a schema file

    Args:
        tmp_path: The temporary directory path
        filename: Name of the Python file to create (e.g., "queries.py") (default: "queries.py")
        function_names: Name(s) of the SQL function(s). Can be a string or list of strings (default: "sql")
        schema_content: Optional SQL schema content (default: None)

    Returns:
        Path to the temporary directory containing the package

    Example:
        package_dir = setup_sqlty_package(tmp_path)
        package_dir = setup_sqlty_package(tmp_path, "db.py", "execute")
        package_dir = setup_sqlty_package(tmp_path, "db.py", ["sql_a", "sql_b"])
        package_dir = setup_sqlty_package(tmp_path, schema_content="CREATE TABLE users (...)")
    """

    def _create_package(
        tmp_path: Path,
        filename: str = "queries.py",
        function_names: str | list[str] = "sql",
        schema_content: str | None = None,
    ) -> Path:
        """Create a complete sqlty package with pyproject.toml."""
        # Normalize function_names to a list
        if isinstance(function_names, str):
            function_names = [function_names]

        # Create the Python file with @sqlty decorator
        py_file = tmp_path / filename
        module_name = filename.replace(".py", "")

        # Generate function definitions
        function_defs = []
        for function_name in function_names:
            function_defs.append(f"""@sqlty
def {function_name}(query: str) -> Any:
    return query
""")

        content = f"""from typing import Any
from sqlty import sqlty


{"".join(function_defs)}"""
        py_file.write_text(content)

        # Create or update pyproject.toml with function configuration
        pyproject = tmp_path / "pyproject.toml"

        # Read existing config if it exists
        if pyproject.exists():
            config_lines = pyproject.read_text().rstrip("\n").split("\n")
        else:
            config_lines = [
                "[tool.sqlty]",
                'dialect = "postgres"',
                "",
            ]

        # Add each function to the config
        for function_name in function_names:
            config_lines.extend(
                [
                    "[[tool.sqlty.functions]]",
                    f'import_path = "{module_name}.{function_name}"',
                    'query_param = "query"',
                ]
            )
            # Add schema if provided (for the first function)
            if schema_content and function_name == function_names[0]:
                schema_file = tmp_path / "schema.sql"
                schema_file.write_text(schema_content)
                config_lines.append('schema = "schema.sql"')
            config_lines.append("")

        pyproject.write_text("\n".join(config_lines))

        return tmp_path

    return _create_package


@pytest.fixture
def create_pyproject_toml() -> Callable[..., Path]:
    """Fixture to create pyproject.toml with function configurations.

    Returns a callable that creates pyproject.toml with the given configurations.

    Args (via returned callable):
        tmp_path: Directory where pyproject.toml will be created
        function_configs: List of (import_path, query_param) tuples. If None, auto-detects from queries.sql
        dialect: SQL dialect (default: postgres)

    Returns:
        A callable function that returns the Path to created pyproject.toml

    Example:
        create_pyproject_toml(tmp_path, [("queries.sql", "query")])
        create_pyproject_toml(tmp_path, [("db.execute", "sql"), ("db.fetchone", "query")])
        create_pyproject_toml(tmp_path)  # Auto-detects queries.sql
    """

    def _create_config(
        tmp_path: Path,
        function_configs: list[tuple[str, str]] | None = None,
        dialect: str = "postgres",
    ) -> Path:
        """Create pyproject.toml with function configurations."""
        pyproject = tmp_path / "pyproject.toml"

        # Auto-detect if not specified
        if function_configs is None:
            function_configs = [("queries.sql", "query")]

        config_lines = [
            "[tool.sqlty]",
            f'dialect = "{dialect}"',
            "",
        ]

        for import_path, query_param in function_configs:
            config_lines.extend(
                [
                    "[[tool.sqlty.functions]]",
                    f'import_path = "{import_path}"',
                    f'query_param = "{query_param}"',
                    "",
                ]
            )

        pyproject.write_text("\n".join(config_lines))
        return pyproject

    return _create_config


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
