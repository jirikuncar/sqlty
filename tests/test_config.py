"""Config lookup: each pyproject.toml is parsed once per run, and edits are picked up."""

from __future__ import annotations

import os

import pytest

from sqlty import _config
from sqlty._config import load_config


def test_parses_each_pyproject_once(tmp_path, monkeypatch):
    """A nested tree of 20 files parses each ancestor pyproject.toml once, not once per file."""
    _config._parse_pyproject_cached.cache_clear()
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
    real_parse = _config._parse_pyproject

    def counting_parse(path):
        parsed.append(str(path))
        return real_parse(path)

    monkeypatch.setattr(_config, "_parse_pyproject", counting_parse)

    results = [load_config(f.parent) for f in files]

    assert all(config["dialect"] == "mysql" for config, _ in results)
    # 1 root + 2 packaging-only dirs + 20 leaves; each is parsed exactly once.
    assert len(parsed) == 1 + 2 + 20
    assert len(set(parsed)) == len(parsed)


def test_edited_pyproject_is_reread(tmp_path):
    """A new mtime_ns on pyproject.toml makes the cached parse stale."""
    _config._parse_pyproject_cached.cache_clear()
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
