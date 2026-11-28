"""Docs agree with the code: rule pages and table match ``RULES``, examples are
rewritten, links resolve, and ``>>>`` examples in the Python API page and in
``sqlty`` docstrings run.

Code blocks with assertions are mdtests, collected by ``tests/_mdtest``.
"""

from __future__ import annotations

import ast
import base64
import doctest
import importlib
import importlib.util
import json
import pkgutil
import re
import subprocess
import sys
import tomllib
import zlib
from pathlib import Path
from typing import Any

import pytest
from pygments.lexers import get_lexer_by_name

import sqlty
from sqlty import _core
from sqlty.core import Settings, check_source, rewrite_source
from tests._core_views import RULES

ROOT = Path(__file__).resolve().parent.parent
RULE_DOCS = ROOT / "docs" / "reference" / "rules"
EXAMPLES = sorted(p for p in (ROOT / "examples").glob("0*") if p.is_dir())
# __main__ runs the CLI on import; _core is compiled and has no docstrings.
# drivers/ is a namespace package, which walk_packages skips.
MODULES = sorted(
    m.name
    for m in pkgutil.walk_packages(sqlty.__path__, "sqlty.")
    if not m.name.endswith("__main__") and m.name != "sqlty._core"
)
ORACLE = sorted(
    m.name
    for m in pkgutil.walk_packages([str(ROOT / "tests" / "type_oracle")], "tests.type_oracle.")
)
WITH_EXAMPLES = {"sqlty", "sqlty.core"}


def test_rules_table() -> None:
    """The table in ``docs/reference/rules.md`` lists every rule with its defaults."""
    text = (ROOT / "docs" / "reference" / "rules.md").read_text()
    rows = re.findall(
        r"^\| \[`([\w-]+)`\]\(rules/\1\.md\) \| ([\w ]+) \| (\w+) \| (\w+) \|", text, re.M
    )
    assert sorted(rows) == sorted(
        (r.name, r.group, r.default_severity, "yes" if r.fixable else "no") for r in RULES.values()
    )


def test_generated_reference_is_current() -> None:
    """The generated tables of ``docs/reference`` match the code (``make generate-docs``)."""
    stale = [p.name for p in _script("generate_docs").generate(check=True)]
    assert not stale, f"run `make generate-docs`: {stale} out of date"


def test_cli_reference_lists_every_command_and_option() -> None:
    from sqlty._core import cli_reference

    page = (ROOT / "docs" / "reference" / "cli.md").read_text()
    for name, usage, options in cli_reference():
        assert name == "sqlty" or f"## `sqlty {name}`" in page, name
        assert f"```text\n{usage}\n```" in page, usage
        for option, _ in options:
            assert f"| `{option}` |" in page, (name, option)


def _script(name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def test_migration_safety_lists_every_migration_rule() -> None:
    text = (ROOT / "docs" / "migration-safety.md").read_text()
    listed = set(re.findall(r"^\| \[`([\w-]+)`\]\(reference/rules/\1\.md\) \|", text, re.M))
    assert listed == {r.name for r in RULES.values() if r.group == "Migration"}


def test_structured_data_dates_are_utc(monkeypatch: pytest.MonkeyPatch) -> None:
    script = _script("structured_data")
    log = "\0" + "2026-11-23T09:42:28+01:00\n\ndocs/faq.md\n"
    monkeypatch.setattr(script, "git", lambda *args: "false" if "rev-parse" in args else log)
    assert script.modified() == {"docs/faq.md": "2026-11-23T08:42:28Z"}


def test_structured_data_groups_rules_in_llms_txt() -> None:
    script = _script("structured_data")
    root = "https://example.com/"
    rule = "- [{0}](<https://example.com/reference/rules/{0}/index.md>): summary\n".format
    text = (
        "## Reference\n\n- [Rules](<https://example.com/reference/rules/index.md>)\n"
        + rule("table-rewrite")
        + rule("unknown-column")
        + rule("missing-concurrently")
        + "\n## Guide\n"
    )
    assert script.group_rules(text, root) == (
        "## Reference\n\n- [Rules](<https://example.com/reference/rules/index.md>)\n"
        "\n## Name, join and schema rules\n\n"
        + rule("unknown-column")
        + "\n## Zero-downtime PostgreSQL migration rules\n\n"
        + rule("missing-concurrently")
        + rule("table-rewrite")
        + "\n## Guide\n"
    )


@pytest.mark.parametrize("page", sorted((ROOT / "docs").rglob("*.md")), ids=lambda p: p.stem)
def test_playground_links_hold_their_example(page: Path) -> None:
    """Each example a docs page links to the playground is whole Python that the playground
    checks as its mdtest does: with the config and schema of its headings."""
    links = _script("playground_links")
    for session in filter(None, links.examples(page.read_text())):
        ast.parse(session["source"])
        assert json.loads(zlib.decompress(base64.urlsafe_b64decode(links.encode(session) + "==")))
        settings = tomllib.loads(session["config"])
        check_source(
            session["source"],
            Settings(
                dialect=session["dialect"],
                schema_sql=session["schema"] or None,
                rules=settings.get("rules"),
                import_style=settings.get("import-style"),
            ),
        )


def test_playground_link_of_a_rule_example() -> None:
    links = _script("playground_links")
    text = (RULE_DOCS / "sqruff.md").read_text()
    session = links.examples(text)[0]
    assert session["config"] == '[rules]\nsqruff = "warn"\n'
    assert session["source"].startswith("from sqlty import SQL\n\nquery = SQL[tuple[str, int]]")
    found = check_source(session["source"], Settings(rules={"sqruff": "warn"}))
    assert [d["rule"] for d in found] == ["sqruff"]
    html = '<div class="language-python highlight"><pre><code>q</code></pre></div>'
    linked = links.link(html, [None, session], "../")
    assert linked == html
    linked = links.link(html * 2, [None, session], "../")
    assert linked.count('href="../playground/#') == 1


def test_readme_playground_links_are_current() -> None:
    links = _script("playground_links")
    text = (ROOT / "README.md").read_text()
    current = {links.encode(session) for session in filter(None, links.examples(text))}
    linked = re.findall(r"https://sqlty\.kuncar\.dev/playground/#([\w-]+)", text)
    assert linked
    assert set(linked) <= current


def test_config_key_docs_cover_every_key() -> None:
    docs = _script("generate_docs")
    assert list(docs.KEY_DOCS) == [*_core.TOOL_KEYS, *_core.DISCOVERY_KEYS]
    assert list(docs.FUNCTION_KEY_DOCS) == list(_core.FUNCTION_KEYS)


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


def test_python_api_table_lists_every_core_function() -> None:
    import sqlty.core

    text = (ROOT / "docs" / "reference" / "python-api.md").read_text().split("## `sqlty.core`")[1]
    documented = set(re.findall(r"^\| `(\w+)\(", text, re.MULTILINE))
    public = {
        name
        for name, value in vars(sqlty.core).items()
        if name.islower()
        and not name.startswith("_")
        and callable(value)
        and getattr(value, "__module__", None) in ("sqlty.core", "sqlty._core")
    }
    assert documented == public
    assert public <= set(sqlty.core.__all__)


@pytest.mark.parametrize(
    "name", ["sqlty", "sqlty.drivers.psycopg", "sqlty.drivers.asyncpg", *MODULES, *ORACLE]
)
def test_module_doctests(name: str, monkeypatch: pytest.MonkeyPatch) -> None:
    """``>>>`` examples in docstrings match what the code returns."""
    monkeypatch.chdir(ROOT)
    module = importlib.import_module(name)
    result = doctest.testmod(module, optionflags=doctest.ELLIPSIS)
    assert result.failed == 0
    assert result.attempted > 0 or name not in WITH_EXAMPLES


def test_pygments_lexer_colors_sql_in_query_strings() -> None:
    lexer = get_lexer_by_name("sqlty")
    source = 'name = "é"\nq = "--sql\\nSELECT \'ü\' FROM users WHERE id = $1"\n'
    tokens = [(str(kind), text) for kind, text in lexer.get_tokens(source) if text.strip()]
    assert "".join(text for _, text in lexer.get_tokens(source)) == source
    assert tokens[2:] == [
        ("Token.Literal.String.Double", '"'),
        ("Token.Literal.String.Double", "é"),
        ("Token.Literal.String.Double", '"'),
        ("Token.Name", "q"),
        ("Token.Operator", "="),
        ("Token.Literal.String.Double", '"'),
        ("Token.Comment.Single", "--sql"),
        ("Token.Literal.String.Escape", "\\n"),
        ("Token.Keyword", "SELECT"),
        ("Token.Literal.String.Single", "'"),
        ("Token.Literal.String.Single", "ü"),
        ("Token.Literal.String.Single", "'"),
        ("Token.Keyword", "FROM"),
        ("Token.Literal.String.Double", " users "),
        ("Token.Keyword", "WHERE"),
        ("Token.Literal.String.Double", " id "),
        ("Token.Operator", "="),
        ("Token.Name.Variable", "$1"),
        ("Token.Literal.String.Double", '"'),
    ]


def test_rule_docs() -> None:
    import yaml

    assert {p.stem for p in RULE_DOCS.glob("*.md")} == set(RULES)
    orders: list[int] = []
    for rule in RULES.values():
        text = (RULE_DOCS / f"{rule.name}.md").read_text()
        _, front, page = text.split("---\n", 2)
        meta = yaml.safe_load(front)
        assert meta["severity"] == rule.default_severity
        assert (meta["fixable"], meta["group"], meta["summary"]) == (
            rule.fixable,
            rule.group.lower().replace(" ", "-"),
            rule.description,
        )
        orders.append(meta["order"])
        assert page.splitlines()[:4] == [
            "",
            f"# {rule.name}",
            "",
            f"Default severity: **{rule.default_severity}**",
        ]
    assert orders == sorted(set(orders))


@pytest.mark.parametrize("example", EXAMPLES, ids=[p.name for p in EXAMPLES])
def test_examples_are_rewritten(example: Path) -> None:
    """Each example is current under its own ``[tool.sqlty]``."""
    config, _, config_file = _core.find_config(example.resolve()) or ({}, [], None)
    config_dir = config_file.parent if config_file else None
    schema = config.get("schema")
    settings = Settings(
        dialect=config.get("dialect", "postgres"),
        schema_sql=(config_dir / schema).read_text() if config_dir and schema else None,
        import_style=config.get("import-style"),
    )
    for path in sorted(example.glob("*.py")):
        source = path.read_text()
        assert rewrite_source(source, settings) == source, f"{path}: run sqlty rewrite"
        stale = [d for d in check_source(source, settings) if str(d["rule"]).startswith("stale-")]
        assert not stale, f"{path}: {stale}"


MARKDOWN = sorted(
    {*ROOT.glob("*.md"), *ROOT.glob(".github/*.md"), *ROOT.glob("skills/*/SKILL.md")}
    | {*ROOT.glob("docs/**/*.md"), *ROOT.glob("examples/*/README.md")}
)


SITE_PAGES = sorted(ROOT.glob("docs/**/*.md"))


@pytest.mark.parametrize("page", SITE_PAGES, ids=[str(p.relative_to(ROOT)) for p in SITE_PAGES])
def test_page_fits_search_results(page: Path) -> None:
    """The <title> and meta description of overrides/main.html fit what search results show:
    60 and 160 characters."""
    import yaml

    text = page.read_text()
    meta = yaml.safe_load(text.split("---\n", 2)[1]) if text.startswith("---\n") else {}
    heading = meta.get("title") or re.search(r"^# (.+)$", text, re.M)[1]
    if page.parent == RULE_DOCS:
        command = "sqlty breaking migration" if meta["group"] == "migration" else "sqlty check"
        title = f"{heading} rule - SQLTy"
        description = (
            f"{heading}: {meta['summary']}. A {command} rule that reports an error by default."
        )
    else:
        title = (
            "SQLTy: type-safe raw SQL for Python"
            if page.name == "index.md" and page.parent.name == "docs"
            else f"{heading} - SQLTy"
        )
        description = meta["description"]
    assert len(title) <= 60
    assert 25 <= len(description) <= 160


@pytest.mark.parametrize("page", MARKDOWN, ids=[str(p.relative_to(ROOT)) for p in MARKDOWN])
def test_links(page: Path) -> None:
    prose = re.sub(r"^(`{3,}).*?^\1", "", page.read_text(), flags=re.M | re.S)
    prose = re.sub(r"`[^`\n]*`", "", prose)
    links = re.findall(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", prose)
    relative = [link for link in links if not re.match(r"[a-z][a-z0-9+.-]*:", link)]
    broken = [link for link in relative if not (page.parent / link).exists()]
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
        "format",
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


def test_every_place_has_the_release_version() -> None:
    """The manifests, `Cargo.lock` and the quoted `rev:` agree with `rust/Cargo.toml`."""
    found = subprocess.run(
        [sys.executable, ROOT / "scripts" / "bump_version.py", "--check"],
        capture_output=True,
        text=True,
    )
    assert found.returncode == 0, f"run `scripts/bump_version.py VERSION`:\n{found.stderr}"


def test_bump_version_writes_every_place(tmp_path: Path) -> None:
    """`bump_version.py X` changes the version lines of every place and no other line."""
    places = {
        "rust/Cargo.toml": 1,
        "rust/Cargo.lock": 10,
        ".claude-plugin/plugin.json": 1,
        ".claude-plugin/marketplace.json": 2,
        ".codex-plugin/plugin.json": 1,
        ".cursor-plugin/plugin.json": 1,
        "docs/recipes/ci.md": 1,
        "skills/sqlty-ci/SKILL.md": 1,
        "npm/sqlty/package.json": 7,
        "npm/wasm/package.json": 1,
        "editors/vscode/package-lock.json": 1,
        "npm/wasm/example/package-lock.json": 1,
    }
    for place in ["scripts/bump_version.py", *places]:
        (tmp_path / place).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / place).write_bytes((ROOT / place).read_bytes())

    def run(*args: str) -> subprocess.CompletedProcess[str]:
        script = tmp_path / "scripts" / "bump_version.py"
        return subprocess.run([sys.executable, script, *args], capture_output=True, text=True)

    old = {place: (tmp_path / place).read_text() for place in places}
    assert run("v9.8.7rc1", "--dry-run").returncode == 0
    assert all((tmp_path / place).read_text() == text for place, text in old.items())

    assert run("v9.8.7rc1").returncode == 0
    for place, count in places.items():
        lines = zip(
            old[place].splitlines(), (tmp_path / place).read_text().splitlines(), strict=True
        )
        changed = [(before, after) for before, after in lines if before != after]
        assert len(changed) == count, place
        assert all(re.sub(r"\d+\.\d+\.\d+", "9.8.7rc1", b) == a for b, a in changed), place
    assert run("--check").returncode == 0

    (tmp_path / "docs/recipes/ci.md").write_text(old["docs/recipes/ci.md"])
    drifted = run("--check")
    assert drifted.returncode == 1
    assert drifted.stderr == "docs/recipes/ci.md: disagrees with rust/Cargo.toml\n"


def test_rule_docs_link_to_the_site(capsys: pytest.CaptureFixture[str]) -> None:
    """`sqlty rule` prints the rule pages with links a terminal user can open."""
    from sqlty._core import cli

    docs_url = tomllib.loads((ROOT / "zensical.toml").read_text())["project"]["site_url"]
    checked = 0

    for name in RULES:
        assert cli(["sqlty", "rule", name]) == 0
        for target in re.findall(r"\]\(([^)\s]+)\)", capsys.readouterr().out):
            assert not target.endswith(".md") and ".md#" not in target, (name, target)
            if target.startswith(docs_url):
                page = target.removeprefix(docs_url).partition("#")[0].rstrip("/")
                assert (ROOT / "docs" / f"{page}.md").exists(), (name, target)
                checked += 1
    assert checked
