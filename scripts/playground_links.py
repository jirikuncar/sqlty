"""Link each Python example of a ``make docs`` build to the playground, with its session.

    python3 scripts/playground_links.py site

An example is read as its mdtest is (``tests/_mdtest``): the ``toml`` config and the schema
files of its heading and the headings above, and the Python blocks before it in its
section back to the last one with assertions. Assertion comments are dropped, since the
playground shows the diagnostics itself. An example that needs what the playground can't
hold, a helper module or a config key that reads files, gets no link.
"""

from __future__ import annotations

import base64
import json
import re
import sys
import tomllib
import zlib
from html import escape
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from tests._mdtest.parser import (  # noqa: E402
    _FENCE,
    _HEADING,
    _LABEL,
    _PY_ASSERTION,
    _TRAILING_ERROR,
    CURSOR,
    _closing_fence,
    _merge,
)

KEYS = {"dialect", "import-style", "param-types", "rules", "schema"}
BLOCK = re.compile(r'<div class="language-(?:python|py) highlight">.*?</code></pre>', re.S)
STYLE = """<style>
  .sqlty-try { display: block; width: fit-content; margin: -.6em 0 1em auto; font-size: .64rem; }
</style>"""


def main() -> None:
    site = Path(sys.argv[1])
    linked = 0
    for page in sorted((ROOT / "docs").rglob("*.md")):
        relative = page.relative_to(ROOT / "docs").with_suffix("")
        directory = relative.parent if relative.name == "index" else relative
        built = site / directory / "index.html"
        if not built.exists() or relative == Path("playground") or "sqlty-try" in built.read_text():
            continue
        html = link(built.read_text(), examples(page.read_text()), "../" * len(directory.parts))
        if count := html.count('class="sqlty-try"'):
            linked += count
            built.write_text(html.replace("</head>", f"{STYLE}</head>", 1))
    print(f"{site}: {linked} examples link to the playground")


def link(html: str, sessions: list[dict[str, str] | None], root: str) -> str:
    """``html`` with a playground link after the ``i``-th Python block that has a session."""
    found = iter(sessions)

    def linked(block: re.Match[str]) -> str:
        session = next(found, None)
        if session is None:
            return block[0]
        href = escape(f"{root}playground/#{encode(session)}")
        title = "Open this example in the playground"
        return f'{block[0]}<a class="sqlty-try" href="{href}" title="{title}">Playground</a>'

    return BLOCK.sub(linked, html)


def examples(text: str) -> list[dict[str, str] | None]:
    """The session of each Python block of a page, in order; ``None`` when it has none."""
    lines = text.splitlines()
    scopes: list[tuple[int, dict[str, Any], dict[str, str]]] = [(0, {}, {})]
    setup: list[str] = []
    found: list[dict[str, str] | None] = []
    label: str | None = None
    i = 0
    while i < len(lines):
        if fence := _FENCE.match(lines[i]):
            indent, marker, lang, info = fence.groups()
            end = _closing_fence(lines, i + 1, indent, marker) or len(lines)
            body = lines[i + 1 : end]
            level, config, files = scopes[-1]
            if label is not None:
                files[label] = "\n".join(body)
                if lang in ("python", "py"):
                    found.append(None)
            elif lang == "toml" and not indent and "ignore" not in info.split():
                tool = tomllib.loads("\n".join(body)).get("tool", {}).get("sqlty", {})
                scopes[-1] = (level, _merge(config, tool), files)
            elif lang in ("python", "py"):
                code = [strip(line) for line in body if not _PY_ASSERTION.match(line)]
                tested = len(code) < len(body) or any(_TRAILING_ERROR.match(line) for line in body)
                if indent or "ignore" in info.split():
                    found.append(None)
                else:
                    found.append(session([*setup, *code], config, files))
                    setup = [] if tested else [*setup, *code]
            label = None
            i = end + 1
            continue
        if heading := _HEADING.match(lines[i]):
            level = len(heading.group(1))
            while scopes[-1][0] >= level:
                scopes.pop()
            _, config, files = scopes[-1]
            scopes.append((level, dict(config), dict(files)))
            setup = []
            label = None
        elif match := _LABEL.match(lines[i]):
            label = match.group(1)
        elif lines[i].strip():
            label = None
        i += 1
    return found


def session(
    code: list[str], config: dict[str, Any], files: dict[str, str]
) -> dict[str, str] | None:
    if set(config) - KEYS or any(name.endswith(".py") for name in files):
        return None
    schema = config.get("schema", [])
    names = [schema] if isinstance(schema, str) else schema
    if not isinstance(names, list) or any(name not in files for name in names):
        return None
    toml = [
        f"{key} = {json.dumps(config[key])}"
        for key in ("dialect", "import-style", "param-types")
        if key in config
    ]
    if rules := config.get("rules"):
        toml += ["", "[rules]", *(f"{rule} = {json.dumps(level)}" for rule, level in rules.items())]
    return {
        "source": "\n".join(code).replace(CURSOR, "").strip("\n") + "\n",
        "schema": "\n".join(files[name] for name in names),
        "dialect": str(config.get("dialect", "postgres")),
        "config": "\n".join(toml).lstrip("\n") + "\n" if toml else "",
    }


def strip(line: str) -> str:
    """``line`` without a trailing ``# error:`` assertion."""
    match = _TRAILING_ERROR.match(line)
    return match.group(1) if match else line


def encode(session: dict[str, str]) -> str:
    """The hash ``share.js`` decodes: the session's JSON, deflated, as base64url."""
    deflated = zlib.compress(json.dumps(session).encode())
    return base64.urlsafe_b64encode(deflated).decode().rstrip("=")


if __name__ == "__main__":
    main()
