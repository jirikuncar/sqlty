"""Docs agree with the code: rule pages and table match ``RULES``, examples are
rewritten, links resolve, and ``>>>`` examples in the Python API page and in
``sqlty`` docstrings run.

Code blocks with assertions are mdtests, collected by ``tests/_mdtest``.
"""

from __future__ import annotations

import doctest
import importlib
import importlib.util
import pkgutil
import re
from pathlib import Path

import pytest

import sqlty
from sqlty._config import load_config
from sqlty._lint import RULES
from sqlty.core import check_source, rewrite_source

ROOT = Path(__file__).resolve().parent.parent
RULE_DOCS = ROOT / "src" / "sqlty" / "rules"
EXAMPLES = sorted(p for p in (ROOT / "examples").glob("0*") if p.is_dir())
# __main__ runs the CLI on import; _core is compiled and has no docstrings.
# drivers/ is a namespace package, which walk_packages skips.
MODULES = sorted(
    m.name
    for m in pkgutil.walk_packages(sqlty.__path__, "sqlty.")
    if not m.name.endswith("__main__") and m.name != "sqlty._core"
)
WITH_EXAMPLES = {"sqlty", "sqlty.core", "sqlty._decode"}


def test_rules_table() -> None:
    """The table in ``docs/reference/rules.md`` lists every rule with its defaults."""
    text = (ROOT / "docs" / "reference" / "rules.md").read_text()
    rows = re.findall(r"^\| \[`([\w-]+)`\]\(#[\w-]+\) \| [\w ]+ \| (\w+) \| (\w+) \|", text, re.M)
    assert sorted(rows) == sorted(
        (r.name, r.default_severity.value, "yes" if r.fixable else "no") for r in RULES.values()
    )


def test_generated_reference_is_current() -> None:
    """The tables in ``configuration.md`` and ``cli.md`` match the code (``make generate-docs``)."""
    spec = importlib.util.spec_from_file_location(
        "generate_docs", ROOT / "scripts" / "generate_docs.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    stale = [p.name for p in module.generate(check=True)]
    assert not stale, f"run `make generate-docs`: {stale} out of date"


def test_config_key_docs_cover_every_key() -> None:
    from sqlty._config import DISCOVERY_KEYS, FUNCTION_KEY_DOCS, FUNCTION_KEYS, KEY_DOCS, TOOL_KEYS

    assert list(KEY_DOCS) == [*TOOL_KEYS, *DISCOVERY_KEYS]
    assert list(FUNCTION_KEY_DOCS) == list(FUNCTION_KEYS)


def test_python_api_doctest() -> None:
    path = ROOT / "docs" / "reference" / "python-api.md"
    # Blank out code fences so a closing ``` is not read as expected output.
    text = "\n".join(
        "" if line.lstrip().startswith("```") else line for line in path.read_text().splitlines()
    )
    test = doctest.DocTestParser().get_doctest(text, {}, path.name, str(path), 0)
    result = doctest.DocTestRunner(optionflags=doctest.ELLIPSIS).run(test)
    assert result.attempted > 0
    assert result.failed == 0


@pytest.mark.parametrize("name", ["sqlty", "sqlty.drivers.psycopg", *MODULES])
def test_module_doctests(name: str, monkeypatch: pytest.MonkeyPatch) -> None:
    """``>>>`` examples in docstrings match what the code returns."""
    monkeypatch.chdir(ROOT)
    module = importlib.import_module(name)
    result = doctest.testmod(module, optionflags=doctest.ELLIPSIS)
    assert result.failed == 0
    assert result.attempted > 0 or name not in WITH_EXAMPLES


def test_rule_docs() -> None:
    assert {p.stem for p in RULE_DOCS.glob("*.md")} == set(RULES)
    for rule in RULES.values():
        head = (RULE_DOCS / f"{rule.name}.md").read_text().splitlines()[:3]
        assert head == [
            f"# {rule.name}",
            "",
            f"Default severity: **{rule.default_severity.value}**",
        ]


@pytest.mark.parametrize("example", EXAMPLES, ids=[p.name for p in EXAMPLES])
def test_examples_are_rewritten(example: Path) -> None:
    """Each example is current under its own ``[tool.sqlty]``."""
    config, config_dir = load_config(example)
    schema = config.get("schema")
    options = {
        "dialect": config.get("dialect", "postgres"),
        "schema_sql": (config_dir / schema).read_text() if config_dir and schema else None,
        "import_style": config.get("import_style"),
    }
    for path in sorted(example.glob("*.py")):
        source = path.read_text()
        assert rewrite_source(source, **options) == source, f"{path}: run sqlty rewrite"
        stale = [d for d in check_source(source, **options) if str(d["rule"]).startswith("stale-")]
        assert not stale, f"{path}: {stale}"


MARKDOWN = sorted(
    {*ROOT.glob("*.md"), *ROOT.glob(".github/*.md"), *ROOT.glob("skills/*/SKILL.md")}
    | {*ROOT.glob("docs/**/*.md"), *RULE_DOCS.glob("*.md"), *ROOT.glob("examples/*/README.md")}
)


@pytest.mark.parametrize("page", MARKDOWN, ids=[str(p.relative_to(ROOT)) for p in MARKDOWN])
def test_links(page: Path) -> None:
    prose = re.sub(r"^(`{3,}).*?^\1", "", page.read_text(), flags=re.M | re.S)
    prose = re.sub(r"`[^`\n]*`", "", prose)
    links = re.findall(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", prose)
    broken = [link for link in links if "://" not in link and not (page.parent / link).exists()]
    assert not broken


@pytest.mark.parametrize(
    "skill", sorted(ROOT.glob("skills/*/SKILL.md")), ids=lambda p: p.parent.name
)
def test_agent_skill_frontmatter(skill: Path) -> None:
    """An Agent Skill follows https://agentskills.io/specification: YAML front matter with its
    folder's name and a description, and a body of at most 500 lines."""
    import yaml

    text = skill.read_text()
    assert text.startswith("---\n"), skill
    _, front, body = text.split("---\n", 2)
    meta = yaml.safe_load(front)
    assert meta["name"] == skill.parent.name
    assert re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*", meta["name"]) and len(meta["name"]) <= 64
    assert 0 < len(meta["description"]) <= 1024
    assert body.count("\n") <= 500
    commands = set(re.findall(r"`sqlty (\w+)", text))
    assert commands <= {
        "rewrite",
        "strip",
        "check",
        "breaking",
        "rule",
        "skill",
        "server",
        "version",
    }, commands


def test_skills_table_lists_every_skill() -> None:
    """The skills table of the agents recipe lists the skills of `skills/`, in order."""
    page = (ROOT / "docs" / "recipes" / "agents.md").read_text()
    table = page.split("<!-- skills-table -->")[1].split("<!-- /skills-table -->")[0]
    listed = re.findall(r"^\| `([a-z0-9-]+)` \|", table, re.M)
    assert listed == sorted(p.parent.name for p in ROOT.glob("skills/*/SKILL.md"))


def test_agent_skills_are_compiled_in() -> None:
    """`sqlty skill` prints the skills of `skills/` as the extension was built with them."""
    from sqlty._core import skills

    files = {
        p.relative_to(ROOT / "skills").as_posix(): p.read_text()
        for p in ROOT.glob("skills/**/*")
        if p.is_file()
    }
    assert dict(skills()) == files, "rebuild the extension: maturin develop"


@pytest.mark.parametrize(
    "manifest",
    [
        ".claude-plugin/plugin.json",
        ".claude-plugin/marketplace.json",
        ".codex-plugin/plugin.json",
        ".cursor-plugin/plugin.json",
    ],
)
def test_plugin_manifests(manifest: str) -> None:
    """Each client's plugin manifest is valid JSON, finds `skills/`, and has the release version."""
    import json
    import tomllib

    version = tomllib.loads((ROOT / "rust" / "Cargo.toml").read_text())["workspace"]["package"][
        "version"
    ]
    data = json.loads((ROOT / manifest).read_text())
    plugins = data.get("plugins", [data])
    assert [plugin["version"] for plugin in plugins] == [version]
    assert [plugin["name"] for plugin in plugins] == ["sqlty"]
    if "plugins" in data:
        assert data["metadata"]["version"] == version
        assert plugins[0]["source"] == "./"
    elif not manifest.startswith(".claude-plugin"):
        assert data["skills"] == "./skills/"


def test_rule_docs_link_to_the_site() -> None:
    """`sqlty rule` prints the rule pages with links a terminal user can open."""
    from sqlty._lint import DOCS_URL, RULES, rule_doc

    for name in RULES:
        for target in re.findall(r"\]\(([^)\s]+)\)", rule_doc(name)):
            assert not target.endswith(".md") and ".md#" not in target, (name, target)
            if target.startswith(DOCS_URL):
                page, _, anchor = target.removeprefix(DOCS_URL).partition("#")
                if page == "reference/rules/":
                    assert anchor in RULES, (name, target)
                else:
                    assert (ROOT / "docs" / f"{page.rstrip('/')}.md").exists(), (name, target)
