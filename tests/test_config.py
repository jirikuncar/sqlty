"""Config lookup: the nearest table wins, and edits are picked up."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from sqlty import _core


def nearest_table(directory: Path) -> tuple[dict, Path | None]:
    """The nearest table and the directory of its file, as the engine reads them (keys as written)."""
    found = _core.find_config(directory.resolve())
    return (found[0], found[2].parent) if found else ({}, None)


def test_packaging_only_pyprojects_inherit_the_nearest_table(tmp_path):
    """Every file of a nested tree of packaging-only pyproject.toml files gets the root's table."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    leaves = []
    for a in range(2):
        (tmp_path / f"a{a}").mkdir()
        (tmp_path / f"a{a}" / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
        for c in range(5):
            leaf = tmp_path / f"a{a}" / f"c{c}"
            leaf.mkdir()
            (leaf / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
            leaves.append(leaf)

    found = [nearest_table(leaf) for leaf in leaves]

    assert all(config["dialect"] == "mysql" for config, _ in found)
    assert {root for _, root in found} == {tmp_path}


def test_edited_pyproject_is_reread(tmp_path):
    """An edited pyproject.toml is read again."""
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text('[tool.sqlty]\ndialect = "mysql"\n')
    assert nearest_table(tmp_path)[0]["dialect"] == "mysql"

    pyproject.write_text('[tool.sqlty]\ndialect = "sqlite"\n')
    stat = pyproject.stat()
    os.utime(pyproject, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))

    assert nearest_table(tmp_path)[0]["dialect"] == "sqlite"


def test_returned_config_is_not_shared(tmp_path):
    """Callers may mutate the returned config without affecting later lookups."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty.rules]\nselect-star = "ignore"\n')
    first, _ = nearest_table(tmp_path)
    first["rules"]["select-star"] = "error"
    assert nearest_table(tmp_path)[0]["rules"] == {"select-star": "ignore"}


@pytest.mark.parametrize("packaging_only", [True, False])
def test_nearest_sqlty_table_wins(tmp_path, packaging_only):
    """A packaging-only pyproject.toml is skipped; the nearest [tool.sqlty] table wins."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    pkg = tmp_path / "pkg"
    pkg.mkdir()
    if packaging_only:
        (pkg / "pyproject.toml").write_text('[project]\nname = "pkg"\n')
        config, config_dir = nearest_table(pkg)
        assert (config["dialect"], config_dir) == ("mysql", tmp_path.resolve())
    else:
        (pkg / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "sqlite"\n')
        config, config_dir = nearest_table(pkg)
        assert (config["dialect"], config_dir) == ("sqlite", pkg.resolve())


def test_sqlty_toml_wins_over_pyproject_in_its_directory(tmp_path: Path) -> None:
    """``sqlty.toml`` holds the keys at the top level and wins over ``pyproject.toml``, like ruff."""
    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    pkg = tmp_path / "pkg"
    pkg.mkdir()
    (pkg / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "postgres"\n')
    (pkg / "sqlty.toml").write_text('dialect = "sqlite"\n[rules]\nselect-star = "ignore"\n')

    config, config_dir = nearest_table(pkg)
    assert (config, config_dir) == (
        {"dialect": "sqlite", "rules": {"select-star": "ignore"}},
        pkg.resolve(),
    )

    (tmp_path / "sqlty.toml").write_text("")
    assert nearest_table(tmp_path) == ({}, tmp_path.resolve())


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

    assert nearest_table(crate) == (
        {"dialect": "sqlite", "rules": {"select-star": "ignore"}},
        tmp_path.resolve(),
    )

    (tmp_path / "pyproject.toml").write_text('[tool.sqlty]\ndialect = "mysql"\n')
    assert nearest_table(crate)[0] == {"dialect": "mysql"}

    (crate / "Cargo.toml").write_text("[package.metadata.sqlty]\nbogus = 1\n")
    with pytest.raises(ValueError, match=r"Cargo\.toml: unknown key 'bogus'"):
        nearest_table(crate)


def test_cargo_workspace_metadata_table_is_read(tmp_path: Path) -> None:
    """A member crate without its own table uses ``[workspace.metadata.sqlty]`` of the workspace root."""
    (tmp_path / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["crate"]\n\n[workspace.metadata.sqlty]\ndialect = "sqlite"\n'
    )
    crate = tmp_path / "crate"
    crate.mkdir()
    (crate / "Cargo.toml").write_text('[package]\nname = "crate"\n')

    assert nearest_table(crate) == ({"dialect": "sqlite"}, tmp_path.resolve())

    (crate / "Cargo.toml").write_text('[package.metadata.sqlty]\ndialect = "mysql"\n')
    assert nearest_table(crate) == ({"dialect": "mysql"}, crate.resolve())

    (tmp_path / "Cargo.toml").write_text(
        '[package.metadata.sqlty]\ndialect = "postgres"\n[workspace.metadata.sqlty]\ndialect = "sqlite"\n'
    )
    assert nearest_table(tmp_path)[0] == {"dialect": "postgres"}


def test_unknown_key_in_sqlty_toml_names_the_file(tmp_path: Path) -> None:
    (tmp_path / "sqlty.toml").write_text("bogus = 1\n")

    with pytest.raises(ValueError, match=r"sqlty\.toml: unknown key 'bogus'"):
        nearest_table(tmp_path)


def test_unknown_keys_raise_config_error_naming_the_file(tmp_path):
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text('[tool.sqlty]\ndialect = "mysql"\nbogus = 1\n')

    with pytest.raises(ValueError, match=r"pyproject\.toml: unknown key 'bogus'"):
        nearest_table(tmp_path)


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
    (tmp_path / "pyproject.toml").write_text(toml)

    with pytest.raises(ValueError, match=message):
        nearest_table(tmp_path)


def test_exclude_settings_are_loaded(tmp_path):
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nexclude = ["a"]\nextend-exclude = ["b"]\n'
    )

    config, _ = nearest_table(tmp_path)

    assert (config["exclude"], config["extend-exclude"]) == (["a"], ["b"])


def test_values_only_the_cli_checks_are_loaded_as_written(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text("[tool.sqlty]\nuntyped = [1.5, 1979-05-27]\n")

    config, _ = nearest_table(tmp_path)

    assert config["untyped"] == [1.5, "1979-05-27"]


def test_rules_must_be_a_table() -> None:
    with pytest.raises(ValueError, match="rules must be a table"):
        _core.validate_config({"rules": ["select-star"]}, "pyproject.toml")


def test_unreadable_pyproject_is_reported_as_a_syntax_error(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text("[tool.sqlty\n")
    with pytest.raises(_core.ConfigSyntaxError, match=r"pyproject\.toml"):
        _core.find_config(tmp_path.resolve())


def test_pyproject_that_is_not_utf8_is_reported_as_a_syntax_error(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_bytes(b"[tool.sqlty]\ndialect = '\xff'\n")
    with pytest.raises(_core.ConfigSyntaxError, match=r"pyproject.toml: invalid utf-8"):
        _core.find_config(tmp_path.resolve())


def test_values_of_a_parsed_table_are_validated() -> None:
    with pytest.raises(ValueError, match="import-style must be a string"):
        _core.validate_config({"import-style": ("a", 1.5)}, "x")
    with pytest.raises(ValueError, match="unknown dialect 'None'"):
        _core.validate_config({"dialect": None}, "x")


def test_tables_keep_their_value_types(tmp_path: Path) -> None:
    table = {"preview": True, "max-query-variants": 8, "rules": {"select-star": "ignore"}}
    _core.validate_config(table, "x")
    (tmp_path / "sqlty.toml").write_text(
        'preview = true\nmax-query-variants = 8\nrules = { select-star = "ignore" }\n'
    )
    config, _ = nearest_table(tmp_path)
    assert config == {"preview": True, "max-query-variants": 8, "rules": {"select-star": "ignore"}}


def test_incomplete_function_entries_are_skipped(tmp_path: Path) -> None:
    (tmp_path / "pyproject.toml").write_text(
        '[tool.sqlty]\nfunctions = ["junk", {import-path = "db.run"},'
        ' {import-path = "db.ok", query-param = "q"}]\n'
    )
    table, skipped, _ = _core.find_config(tmp_path.resolve())
    assert table["functions"] == [{"import-path": "db.ok", "query-param": "q"}]
    assert len(skipped) == 2


def test_unknown_dialect_is_an_error_with_a_suggestion() -> None:
    with pytest.raises(ValueError, match=r"unknown dialect 'postgress'.*did you mean 'postgres'"):
        _core.validate_config({"dialect": "postgress"}, "pyproject.toml")
    _core.validate_config({"dialect": "PostgreSQL"}, "pyproject.toml")


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

    assert nearest_table(pkg / "src") == (
        {
            "dialect": "mysql",
            "schema": ["../../db/*.sql"],
            "queries": ["../../sql"],
            "functions": [{"import-path": "billing.run", "query-param": "sql"}],
            "rules": {"select-star": "ignore", "stale-type": "error"},
            "types": {"DATE": "str", "UUID": "str"},
        },
        pkg.resolve(),
    )

    (pkg / "sqlty.toml").write_text('extend = "../../pyproject.toml"\nschema = "local.sql"\n')
    assert nearest_table(pkg)[0]["schema"] == "local.sql"


def test_extend_merges_databases_by_name_and_rebases_their_paths(tmp_path: Path) -> None:
    (tmp_path / "sqlty.toml").write_text(
        '[databases.a]\nschema = "a.sql"\ninclude = ["x"]\n[databases.b]\ndialect = "mysql"\n'
    )
    (tmp_path / "pkg").mkdir()
    (tmp_path / "pkg" / "sqlty.toml").write_text(
        'extend = "../sqlty.toml"\n[databases.b]\nqueries = "sql"\n'
    )

    assert nearest_table(tmp_path / "pkg")[0] == {
        "databases": {
            "a": {"schema": ["../a.sql"], "include": ["x"], "exclude": []},
            "b": {"queries": "sql", "include": [], "exclude": []},
        }
    }


def test_extend_follows_a_chain_and_rereads_an_edited_base(tmp_path: Path) -> None:
    base = tmp_path / "base.toml"
    base.write_text('dialect = "sqlite"\n')
    (tmp_path / "mid.toml").write_text('extend = "base.toml"\nimport-style = "import sqlty"\n')
    (tmp_path / "sqlty.toml").write_text('extend = "mid.toml"\n')

    assert nearest_table(tmp_path)[0] == {"dialect": "sqlite", "import-style": "import sqlty"}

    base.write_text('dialect = "mysql"\n')
    stat = base.stat()
    os.utime(base, ns=(stat.st_atime_ns, stat.st_mtime_ns + 1_000_000_000))
    assert nearest_table(tmp_path)[0]["dialect"] == "mysql"


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

    with pytest.raises(ValueError, match=message):
        nearest_table(tmp_path)
