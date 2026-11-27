"""rust/fuzz is a workspace of its own, so nothing makes it follow rust/Cargo.toml."""

from __future__ import annotations

import tomllib
from pathlib import Path

RUST = Path(__file__).resolve().parent.parent / "rust"


def test_the_fuzz_crate_pins_the_versions_of_the_workspace() -> None:
    workspace = tomllib.loads((RUST / "Cargo.toml").read_text())["workspace"]["dependencies"]
    fuzz = tomllib.loads((RUST / "fuzz" / "Cargo.toml").read_text())
    pinned = {**fuzz["dependencies"], **fuzz["dev-dependencies"]}

    def version(spec: str | dict[str, str]) -> str | None:
        return spec if isinstance(spec, str) else spec.get("version")

    shared = sorted(set(pinned) & set(workspace))
    assert shared
    assert {name: version(pinned[name]) for name in shared} == {
        name: version(workspace[name]) for name in shared
    }
