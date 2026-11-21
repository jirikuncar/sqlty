"""Configuration loading from sqlty.toml, pyproject.toml or Cargo.toml."""

import logging
from pathlib import Path
from typing import Any

from sqlty import _core

Config = dict[str, Any]
"""A ``[tool.sqlty]`` table, its keys in snake_case."""

TOOL_KEYS = _core.TOOL_KEYS
"""Keys of ``[tool.sqlty]`` that are not file discovery; see also ``DISCOVERY_KEYS``."""

DISCOVERY_KEYS = _core.DISCOVERY_KEYS
"""``[tool.sqlty]`` keys that pick the files to process."""

FUNCTION_KEYS = _core.FUNCTION_KEYS


DEFAULT_EXCLUDE: tuple[str, ...] = tuple(_core.DEFAULT_EXCLUDE)
"""Directories skipped when walking a directory, unless ``exclude`` replaces them."""


class ConfigError(Exception):
    """A user error in configuration or command-line paths; the CLI prints it and exits 2."""


def load_config(project_root: Path | None = None) -> tuple[Config, Path | None]:
    """Load the nearest configuration above ``project_root``; see ``find_config``.

    Returns:
        Tuple of (configuration dictionary, directory containing its file, or None
        if none was found or a native file failed to parse)
    """
    config, path = find_config(project_root)
    return config, path.parent if path else None


def find_config(project_root: Path | None = None) -> tuple[Config, Path | None]:
    """Find the nearest ``sqlty.toml``, ``[tool.sqlty]`` of ``pyproject.toml`` or
    ``[package.metadata.sqlty]`` (else ``[workspace.metadata.sqlty]``) of ``Cargo.toml``,
    and the file it came from.

    Walks up from ``project_root`` (cwd if None). In each directory the files are
    tried in that order, and a native file without its sqlty table is skipped (like
    ruff), so a packaging-only ``pyproject.toml`` or ``Cargo.toml`` does not hide the
    workspace config. The closest file wins; files are never merged, except through
    ``extend``. A native file that fails to parse is logged and yields ``({}, None)``.
    """
    try:
        found = _core.find_config((project_root or Path.cwd()).resolve())
    except _core.ConfigSyntaxError as e:
        logging.getLogger(__name__).warning("Failed to parse %s", e)
        return {}, None
    except ValueError as exc:
        raise ConfigError(str(exc)) from None
    if found is None:
        return {}, None
    table, skipped, path = found
    return _converted(table, skipped, str(path)), path


def _converted(table: dict[str, Any], skipped: list[str], source: str) -> Config:
    if "databases" in table:
        table["databases"] = {
            name: _converted(database, [], source) for name, database in table["databases"].items()
        }
    for entry in skipped:
        logging.getLogger(__name__).warning(
            "Skipping function config missing required fields: %s", entry
        )
    if "functions" in table:
        table["functions"] = [
            {key.replace("-", "_"): value for key, value in function.items()}
            for function in table["functions"]
        ]
    return {key.replace("-", "_"): value for key, value in table.items()}
