"""Write ``docs/assets/playground/examples.json``: the projects of ``examples/`` as
playground sessions, for its Examples select.

    uv run python scripts/generate_playground_examples.py

Each numbered project gives its ``README.md`` title, ``queries.py``, ``schema.sql`` and the
``[tool.sqlty]`` keys the playground reads (``dialect``, ``import-style``, ``rules``) as a
``sqlty.toml``. ``make playground`` runs it; the output is not committed.
"""

from __future__ import annotations

import json
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "docs" / "assets" / "playground" / "examples.json"


def main() -> None:
    examples = []
    for project in sorted(ROOT.glob("examples/[0-9]*/")):
        pyproject = project / "pyproject.toml"
        tool = tomllib.loads(pyproject.read_text())["tool"]["sqlty"] if pyproject.exists() else {}
        config = [
            f"{key} = {json.dumps(tool[key])}" for key in ("dialect", "import-style") if key in tool
        ]
        if rules := tool.get("rules"):
            config += [
                "",
                "[rules]",
                *(f"{rule} = {json.dumps(level)}" for rule, level in rules.items()),
            ]
        schema = project / "schema.sql"
        examples.append(
            {
                "name": (project / "README.md").read_text().splitlines()[0].removeprefix("# "),
                "source": (project / "queries.py").read_text(),
                "schema": schema.read_text() if schema.exists() else "",
                "dialect": tool.get("dialect", "postgres"),
                "config": "\n".join(config) + "\n" if config else "",
            }
        )
    OUTPUT.write_text(json.dumps(examples, indent=2) + "\n")


if __name__ == "__main__":
    main()
