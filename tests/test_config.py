"""Config lookup: each pyproject.toml is parsed once per run, and edits are picked up."""

from __future__ import annotations

import logging
import os
from pathlib import Path

import pytest

from sqlty import _config
from sqlty._config import ConfigError, load_config, validate_tool_table


def test_parses_each_pyproject_once(tmp_path, monkeypatch):
    """A nested tree of 20 files parses each ancestor pyproject.toml once, not once per file."""
    _config._parse_native_cached.cache_clear()
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    files = []
    for a in range(2):
        for b in range(2):
            for c in range(5):
                leaf = tmp_path / f"a{a}" / f"b{b}" / f"c{c}"
                leaf.mkdir(parents=True)
                (leaf / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
                files.append(leaf / "q.py")
    for a in range(2):
        (tmp_path / f"a{a}" / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
    for i in range(20):
        files[i].write_text("")

    parsed: list[str] = []
    real_parse = _config._parse_native

    def counting_parse(path):
        parsed.append(str(path))
        return real_parse(path)

    monkeypatch.setattr(_config, "_parse_native", counting_parse)

    results = [load_config(f.parent) for f in files]

    assert all(config["dialect"] == "mysql" for config, _ in results)
    # 1 root + 2 packaging-only dirs + 20 leaves; each is parsed exactly once.
    assert len(parsed) == 1 + 2 + 20
    assert len(set(parsed)) == len(parsed)


def test_edited_pyproject_is_reread(tmp_path):
    """A new mtime_ns on pyproject.toml makes the cached parse stale."""
    _config._parse_native_cached.cache_clear()
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text('[tool.sqlty]\ndialect = "mysql"\n')
    assert load_config(tmp_path)[0]["dialect"] == "mysql"

    pyproject.write_text('[tool.sqlty]\ndialect = "sqlite"\n')
    stat = pyproject.stat()
    os.utime(pyproject, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))

    assert load_config(tmp_path)[0]["dialect"] == "sqlite"


def test_returned_config_is_not_shared(tmp_path):
    """Callers may mutate the returned config without affecting later lookups."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty.rules]\nselect-star = "ignore"\n')
    first, _ = load_config(tmp_path)
    first["rules"]["select-star"] = "error"
    assert load_config(tmp_path)[0]["rules"] == {"select-star": "ignore"}


@pytest.mark.parametrize("packaging_only", [True, False])
def test_nearest_sqlty_table_wins(tmp_path, packaging_only):
    """A packaging-only pyproject.toml is skipped; the nearest [tool.sqlty] table wins."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    pkg = tmp_path / "pkg"
    pkg.mkdir()
    if packaging_only:
        (pkg / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
        config, config_dir = load_config(pkg)
        assert (config["dialect"], config_dir) == ("mysql", tmp_path.resolve())
    else:
        (pkg / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "sqlite"\n')
        config, config_dir = load_config(pkg)
        assert (config["dialect"], config_dir) == ("sqlite", pkg.resolve())


def test_sqlty_toml_wins_over_pyproject_in_its_directory(tmp_path: Path) -> None:
    """``sqlty.toml`` holds the keys at the top level and wins over ``pyproject.toml``, like ruff."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    pkg = tmp_path / "pkg"
    pkg.mkdir()
    (pkg / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "postgres"\n')
    (pkg / "sqlty.toml").write_text('dialect = "sqlite"\n[rules]\nselect-star = "ignore"\n')

    config, config_dir = load_config(pkg)
    assert (config, config_dir) == (
        {"dialect": "sqlite", "rules": {"select-star": "ignore"}},
        pkg.resolve(),
    )

    (tmp_path / "sqlty.toml").write_text("")
    assert load_config(tmp_path) == ({}, tmp_path.resolve())


def test_cargo_toml_metadata_table_is_read(tmp_path: Path) -> None:
    """``[package.metadata.sqlty]`` configures a crate; a ``Cargo.toml`` without it is skipped."""
    (tmp_path / "Cargo.toml").write_text(
        '[package]\nname = "app"\n\n[package.metadata.sqlty]\ndialect = "sqlite"\n'
        '[package.metadata.sqlty.rules]\nselect-star = "ignore"\n'
    )
    crate = tmp_path / "crate"
    crate.mkdir()
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "crate"\n[package.metadata.docs]\nall = true\n'
    )

    assert load_config(crate) == (
        {"dialect": "sqlite", "rules": {"select-star": "ignore"}},
        tmp_path.resolve(),
    )

    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    assert load_config(crate)[0] == {"dialect": "mysql"}

    (crate / "Cargo.toml").write_text("[package.metadata.sqlty]\nbogus = 1\n")
    with pytest.raises(ConfigError, match=r"Cargo\.toml: unknown key 'bogus'"):
        load_config(crate)


def test_cargo_workspace_metadata_table_is_read(tmp_path: Path) -> None:
    """A member crate without its own table uses ``[workspace.metadata.sqlty]`` of the workspace root."""
    (tmp_path / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["crate"]\n\n[workspace.metadata.sqlty]\ndialect = "sqlite"\n'
    )
    crate = tmp_path / "crate"
    crate.mkdir()
    (crate / "Cargo.toml").write_text('[package]\nname = "crate"\n')

    assert load_config(crate) == ({"dialect": "sqlite"}, tmp_path.resolve())

    (crate / "Cargo.toml").write_text('[package.metadata.sqlty]\ndialect = "mysql"\n')
    assert load_config(crate) == ({"dialect": "mysql"}, crate.resolve())

    (tmp_path / "Cargo.toml").write_text(
        '[package.metadata.sqlty]\ndialect = "postgres"\n[workspace.metadata.sqlty]\ndialect = "sqlite"\n'
    )
    assert load_config(tmp_path)[0] == {"dialect": "postgres"}


def test_unknown_key_in_sqlty_toml_names_the_file(tmp_path: Path) -> None:
    (tmp_path / "sqlty.toml").write_text("bogus = 1\n")

    with pytest.raises(ConfigError, match=r"sqlty\.toml: unknown key 'bogus'"):
        load_config(tmp_path)


def test_unknown_keys_raise_config_error_naming_the_file(tmp_path):
    _config._parse_native_cached.cache_clear()
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text('[tool.sqlty]\ndialect = "mysql"\nbogus = 1\n')

    with pytest.raises(_config.ConfigError, match=r"pyproject\.toml: unknown key 'bogus'"):
        load_config(tmp_path)


@pytest.mark.parametrize(
    ("toml", "message"),
    [
        ("[tool.sqlty.rules]\nnope = 'warn'\n", "unknown rule 'nope'"),
        ("[tool.sqlty.rules]\nstale-typ = 'warn'\n", "did you mean 'stale-type'?"),
        ("[tool.sqlty.rules]\nstale-type = 'loud'\n", "invalid severity 'loud'"),
        ("[[tool.sqlty.functions]]\nimport-path = 'a.b'\nquery = 'q'\n", "unknown key 'query'"),
        (
            "[[tool.sqlty.functions]]\nimport_path = 'a.b'\nquery-param = 'q'\n",
            "did you mean 'import-path'?",
        ),
        ("[tool.sqlty]\nexclude = 'build'\n", "exclude must be a list of glob strings"),
        ("[tool.sqlty]\ntypes = 'DATE'\n", "types must be a table"),
        ("[tool.sqlty.types]\nDATE = 1\n", "types must be a table"),
        ("[tool.sqlty.types]\nDATE = 'list['\n", "is not a Python type expression"),
        ("[tool.sqlty]\npreview = 'yes'\n", "preview must be true or false"),
        ("[tool.sqlty]\nmax-query-variants = 0\n", "max-query-variants must be a positive integer"),
        (
            "[tool.sqlty]\nmax-query-variants = true\n",
            "max-query-variants must be a positive integer",
        ),
        ("[tool.sqlty]\nimport-style = 1\n", "import-style must be a string"),
        ("[tool.sqlty]\nschema = [1]\n", "schema must be a path or a list of paths"),
        ("[tool.sqlty]\noverrides = {}\n", "overrides must be an array of tables"),
        ("[[tool.sqlty.overrides]]\nrule = {}\n", "did you mean 'rules'?"),
        ("[tool.sqlty]\nfunctions = {}\n", "functions must be an array of tables"),
        (
            "[[tool.sqlty.functions]]\nimport-path = 1\nquery-param = 'q'\n",
            "functions]] import-path must be a string",
        ),
    ],
)
def test_invalid_settings(tmp_path, toml, message):
    _config._parse_native_cached.cache_clear()
    (tmp_path / "pyproject.toml").write_text(toml)

    with pytest.raises(_config.ConfigError, match=message):
        load_config(tmp_path)


def test_exclude_settings_are_loaded(tmp_path):
    _config._parse_native_cached.cache_clear()
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nexclude = ["a"]\nextend-exclude = ["b"]\n'
    )

    config, _ = load_config(tmp_path)

    assert (config["exclude"], config["extend_exclude"]) == (["a"], ["b"])


def test_rules_must_be_a_table() -> None:
    with pytest.raises(ConfigError, match="rules must be a table"):
        validate_tool_table({"rules": ["select-star"]}, "pyproject.toml")


def test_unreadable_pyproject_is_skipped_with_a_warning(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    (tmp_path / "pyproject.toml").write_text("[tool.sqlty\n")
    with caplog.at_level(logging.WARNING):
        config, root = load_config(tmp_path)
    assert (config, root) == ({}, None)
    assert "Failed to parse" in caplog.text


def test_pyproject_that_is_not_utf8_is_skipped_with_a_warning(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    (tmp_path / "pyproject.toml").write_bytes(b"[tool.sqlty]\ndialect = '\xff'\n")
    with caplog.at_level(logging.WARNING):
        assert load_config(tmp_path) == ({}, None)
    assert "pyproject.toml: invalid utf-8" in caplog.text


def test_values_of_a_parsed_table_are_validated() -> None:
    with pytest.raises(ConfigError, match="import-style must be a string"):
        validate_tool_table({"import-style": ("a", 1.5)}, "x")
    with pytest.raises(ConfigError, match="unknown dialect 'None'"):
        validate_tool_table({"dialect": None}, "x")


def test_incomplete_function_entries_are_skipped(
    tmp_path: Path, caplog: pytest.LogCaptureFixture
) -> None:
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nfunctions = ["junk", {import-path = "db.run"},'
        ' {import-path = "db.ok", query-param = "q"}]\n'
    )
    with caplog.at_level(logging.WARNING):
        config, _ = load_config(tmp_path)
    assert config["functions"] == [{"import_path": "db.ok", "query_param": "q"}]
    assert "missing required fields" in caplog.text


def test_load_config_file_reads_top_level_keys(tmp_path: Path) -> None:
    from sqlty._config import load_config_file

    path = tmp_path / "sqlty.toml"
    path.write_text(
        'schema = ["a.sql"]\nextend-exclude = ["gen"]\n[rules]\nselect-star = "ignore"\n'
    )

    assert load_config_file(path) == {
        "schema": ["a.sql"],
        "extend_exclude": ["gen"],
        "rules": {"select-star": "ignore"},
    }


def test_unknown_dialect_is_an_error_with_a_suggestion() -> None:
    with pytest.raises(ConfigError, match=r"unknown dialect 'postgress'.*did you mean 'postgres'"):
        validate_tool_table({"dialect": "postgress"}, "pyproject.toml")
    validate_tool_table({"dialect": "PostgreSQL"}, "pyproject.toml")


def test_extend_applies_a_package_config_on_top_of_another(tmp_path: Path) -> None:
    """``rules`` and ``types`` merge key by key, other keys replace, extended paths are rebased."""
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\ndialect = "mysql"\nschema = ["db/*.sql"]\nqueries = "sql"\n'
        "functions = [{ import-path = 'db.run', query-param = 'q' }]\n"
        '[tool.sqlty.rules]\nselect-star = "ignore"\nstale-type = "warn"\n'
        '[tool.sqlty.types]\nDATE = "str"\n'
    )
    pkg = tmp_path / "packages" / "billing"
    pkg.mkdir(parents=True)
    (pkg / "sqlty.toml").write_text(
        'extend = "../../pyproject.toml"\n'
        "functions = [{ import-path = 'billing.run', query-param = 'sql' }]\n"
        '[rules]\nstale-type = "error"\n[types]\nUUID = "str"\n'
    )

    assert load_config(pkg / "src") == (
        {
            "dialect": "mysql",
            "schema": ["../../db/*.sql"],
            "queries": ["../../sql"],
            "functions": [{"import_path": "billing.run", "query_param": "sql"}],
            "rules": {"select-star": "ignore", "stale-type": "error"},
            "types": {"DATE": "str", "UUID": "str"},
        },
        pkg.resolve(),
    )

    (pkg / "sqlty.toml").write_text('extend = "../../pyproject.toml"\nschema = "local.sql"\n')
    assert load_config(pkg)[0]["schema"] == "local.sql"


def test_extend_merges_databases_by_name_and_rebases_their_paths(tmp_path: Path) -> None:
    (tmp_path / "sqlty.toml").write_text(
        '[databases.a]\nschema = "a.sql"\ninclude = ["x"]\n[databases.b]\ndialect = "mysql"\n'
    )
    (tmp_path / "pkg").mkdir()
    (tmp_path / "pkg" / "sqlty.toml").write_text(
        'extend = "../sqlty.toml"\n[databases.b]\nqueries = "sql"\n'
    )

    assert load_config(tmp_path / "pkg")[0] == {
        "databases": {
            "a": {"schema": ["../a.sql"], "include": ["x"], "exclude": []},
            "b": {"queries": "sql", "include": [], "exclude": []},
        }
    }


def test_extend_follows_a_chain_and_rereads_an_edited_base(tmp_path: Path) -> None:
    from sqlty._config import load_config_file

    base = tmp_path / "base.toml"
    base.write_text('dialect = "sqlite"\n')
    (tmp_path / "mid.toml").write_text('extend = "base.toml"\nimport-style = "import sqlty"\n')
    (tmp_path / "sqlty.toml").write_text('extend = "mid.toml"\n')

    assert load_config(tmp_path)[0] == {"dialect": "sqlite", "import_style": "import sqlty"}
    assert load_config_file(tmp_path / "sqlty.toml") == load_config(tmp_path)[0]

    base.write_text('dialect = "mysql"\n')
    stat = base.stat()
    os.utime(base, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))
    assert load_config(tmp_path)[0]["dialect"] == "mysql"


@pytest.mark.parametrize(
    ("files", "message"),
    [
        ({"sqlty.toml": 'extend = "missing.toml"\n'}, "config file does not exist"),
        ({"sqlty.toml": 'extend = "sqlty.toml"\n'}, "extends itself in a cycle"),
        (
            {"sqlty.toml": 'extend = "a.toml"\n', "a.toml": 'extend = "sqlty.toml"\n'},
            "extends itself in a cycle",
        ),
        ({"sqlty.toml": "extend = 1\n"}, "extend must be a string"),
        ({"sqlty.toml": 'extend = "a.toml"\n', "a.toml": "bogus = 1\n"}, "unknown key 'bogus'"),
    ],
)
def test_invalid_extend_is_an_error(tmp_path: Path, files: dict[str, str], message: str) -> None:
    for name, content in files.items():
        (tmp_path / name).write_text(content)

    with pytest.raises(ConfigError, match=message):
        load_config(tmp_path)
