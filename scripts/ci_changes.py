"""Which CI packages a change affects, with every package that depends on one of them.

Each Rust crate is a package of its own; its workspace dependencies come from its
Cargo.toml. A file no package owns runs every package, unless it is listed in IGNORED.
A change to rust/Cargo.lock runs the crates whose dependencies it changes.

    python3 scripts/ci_changes.py [BASE]   # BASE omitted: every package

Writes `crates` (the affected crates' directories, space-separated), `packages` (their
package names) and one `true`/`false` per other package to $GITHUB_OUTPUT, or prints them.
"""

from __future__ import annotations

import fnmatch
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "rust" / "crates"

# What every pytest run loads: the package, the configuration and its plugins.
PYTEST = [
    "src/**",
    "pyproject.toml",
    "uv.lock",
    "tests/conftest.py",
    "tests/_mdtest/**",
    "tests/_no_exec.py",
]
# The Markdown of the guides, at the root and in docs/, and the Vale styles that judge it.
DOCS = ["docs/**.md", "*.md", ".github/*.md", ".vale.ini", ".vale/**"]
# The tests of the docs job besides the mdtests of docs/ and README.md: each reads the guides.
DOCS_TESTS = [
    "tests/test_docs.py",
    "tests/test_core_invariants.py",
    "tests/test_duplicates.py",
    "tests/test_rust_types.py",
]
# The Markdown whose queries the database tests run.
CORPUS = ["tests/mdtest/**", "docs/**.md"]

# Paths a package owns, and the packages it depends on beyond its own crates. Each is an
# output the jobs of tests.yml, docs.yml, vscode.yml or compat.yml are gated on.
PACKAGES: dict[str, tuple[list[str], list[str]]] = {
    # The extension is tested through the Python package, which reads most of the repository.
    "python": (
        [
            "src/**",
            "tests/**",
            "docs/assets/**",
            "docs/favicon.ico",
            "docs/robots.txt",
            "skills/**",
            "examples/**",
            "scripts/**",
            "benchmarks/**",
            "LICENSE",
            "Makefile",
            "pyproject.toml",
            "uv.lock",
            "prek.toml",
            ".pre-commit-hooks.yaml",
            ".claude-plugin/**",
            ".codex-plugin/**",
            ".cursor-plugin/**",
            ".github/workflows/pypi.yml",
            "npm/sqlty/package.json",
        ],
        ["sqlty-ext"],
    ),
    # The jobs that build and run the engine itself, each on the files it reads besides the
    # extension: a change to another test or to the docs alone skips them.
    "free-threaded": ([*PYTEST, "tests/test_threads.py"], ["sqlty-ext"]),
    "pg-fidelity": (
        [
            *PYTEST,
            *CORPUS,
            "tests/test_pg_describe.py",
            "tests/test_pg_common_types.py",
            "tests/test_driver_values.py",
            "tests/_pg_seed.py",
            "tests/_value_types.py",
            "tests/fixtures/pg_common_types.toml",
            "tests/fixtures/driver_values.toml",
        ],
        ["sqlty-ext"],
    ),
    "mysql-fidelity": (
        [
            *PYTEST,
            *CORPUS,
            "tests/test_server_describe.py",
            "tests/test_driver_values.py",
            "tests/_value_types.py",
            "tests/fixtures/driver_values.toml",
        ],
        ["sqlty-ext"],
    ),
    # The performance budget and CodSpeed.
    "bench": ([*PYTEST, "benchmarks/**", "Makefile"], ["sqlty-ext"]),
    # The wheel and sdist, which carry the Agent Skills.
    "package": (
        [
            "src/**",
            "skills/**",
            "pyproject.toml",
            "uv.lock",
            "README.md",
            "LICENSE",
            "tests/check_dist.py",
            ".github/workflows/pypi.yml",
        ],
        ["sqlty-ext"],
    ),
    # The guides: the tests that read them (links, rule pages, examples) and their mdtests.
    "docs": ([*DOCS, *PYTEST, *DOCS_TESTS], ["sqlty-ext"]),
    # The desktop test runs the sqlty of the virtual environment; vscode.yml packages it.
    "vscode": (
        ["editors/vscode/**", "src/sqlty/**", "npm/wasm/**", ".github/workflows/vscode.yml"],
        ["sqlty-wasm", "sqlty-ext"],
    ),
    # The engine on WebAssembly: the docs playground and @sqlty/wasm, checking each example.
    "playground": (
        [
            "docs/assets/playground/**",
            "npm/wasm/**",
            "tests/*.test.mjs",
            "examples/**",
            "scripts/generate_playground_examples.py",
            "scripts/vendor_assets.py",
            "Makefile",
        ],
        ["sqlty-wasm", "sqlty-lsp"],
    ),
    # Workflows of their own (docs.yml, compat.yml), gated on these outputs too.
    # The site: the docs with the playground, built by zensical and colored by the lexer.
    "site": (
        [
            "docs/**",
            "examples/**",
            "npm/wasm/**",
            "overrides/**",
            "zensical.toml",
            "wrangler.jsonc",
            "Makefile",
            "pyproject.toml",
            "uv.lock",
            # The lexer imports the package's core.
            "src/sqlty/lexer.py",
            "src/sqlty/core.py",
            "src/sqlty/__init__.py",
            "tests/_mdtest/parser.py",
            "scripts/generate_playground_examples.py",
            "scripts/vendor_assets.py",
            "scripts/offline.py",
            "scripts/playground_links.py",
            "scripts/structured_data.py",
            "scripts/check_playground.py",
            ".github/workflows/docs.yml",
            ".github/workflows/docs-preview-cleanup.yml",
        ],
        ["sqlty-wasm", "sqlty-ext"],
    ),
    # The comparisons with drivers and ORMs, at the versions scripts/compat/ pins.
    "compat": (["scripts/compat/**", ".github/workflows/compat.yml"], []),
}

# Files outside a crate's directory that its build or tests read.
CRATE_PATHS = {
    "sqlty-cli": ["skills/**", "docs/reference/rules/**"],
    "sqlty-rules": ["docs/reference/rules/**"],
    "sqlty-sql": ["editors/vscode/syntaxes/**"],
}

# Files only a crate's tests read: a change runs those tests, not the crates that use it.
MDTEST = ["rust/mdtest/**", "tests/mdtest/**", "docs/*.md", "README.md", "skills/**"]
TEST_DATA = {"sqlty-sql": MDTEST, "sqlty-python": MDTEST, "sqlty-format": MDTEST}

# A change here runs every package.
EVERYTHING = [
    ".github/workflows/tests.yml",
    ".github/actions/**",
    "scripts/ci_changes.py",
    "scripts/coverage.sh",
    "scripts/coverage-floors.txt",
    "scripts/slowest.py",
    "rust/.config/**",
    "rust/Cargo.toml",
    "rust/clippy.toml",
    "rust-toolchain.toml",
]

# Tested by nothing in tests.yml: other workflows own these, or nothing does.
IGNORED = [
    "plans/**",
    ".claude/**",
    ".github/**",
    "npm/**",
    "overrides/**",
    "rust/fuzz/**",
    "zensical.toml",
    "wrangler.jsonc",
    "context7.json",
    ".gitignore",
    # The interpreter uv picks locally and the editor's recommendations: CI names its own.
    ".python-version",
    ".vscode/**",
]


def workspace() -> dict[str, tuple[str, set[str]]]:
    """Each crate's directory with its package name and the directories of the crates it uses."""
    manifests = {
        path.parent.name: tomllib.loads(path.read_text()) for path in CRATES.glob("*/Cargo.toml")
    }
    directory = {data["package"]["name"]: name for name, data in manifests.items()}
    graph = {}
    for name, data in manifests.items():
        tables = [
            data.get(k, {}) for k in ("dependencies", "dev-dependencies", "build-dependencies")
        ]
        for target in data.get("target", {}).values():
            tables += [target.get(k, {}) for k in ("dependencies", "dev-dependencies")]
        deps = {directory[dep] for table in tables for dep in table if dep in directory}
        graph[name] = (data["package"]["name"], deps)
    return graph


def matches(path: str, patterns: list[str]) -> bool:
    """Whether `path` matches a pattern; one without a `/` matches files at the root only."""
    return any(
        fnmatch.fnmatchcase(path, pattern) and ("/" in pattern or "/" not in path)
        for pattern in patterns
    )


def owners(path: str, crates: set[str]) -> set[str] | None:
    """The packages that own `path`: empty when ignored, None when nobody does."""
    if matches(path, EVERYTHING):
        return None
    parts = path.split("/")
    if parts[:2] == ["rust", "crates"] and len(parts) > 3 and parts[2] in crates:
        return {parts[2]}
    found = {name for name, (paths, _) in PACKAGES.items() if matches(path, paths)}
    found |= {name for name, paths in CRATE_PATHS.items() if matches(path, paths)}
    if found or matches(path, IGNORED) or data(path):
        return found
    return None


def data(path: str) -> set[str]:
    return {name for name, paths in TEST_DATA.items() if matches(path, paths)}


def affected(changed: list[str] | None, locked: set[str] | None = None) -> dict[str, bool]:
    """The packages `changed` reaches; `locked` are the crates a change to rust/Cargo.lock
    reaches, as `locked_crates` gives them (None: every package)."""
    graph = {name: deps for name, (_, deps) in workspace().items()}
    graph.update({name: set(deps) for name, (_, deps) in PACKAGES.items()})
    everything = set(graph)
    if changed is None:
        return dict.fromkeys(sorted(everything), True)

    hit: set[str] = set()
    for path in changed:
        if path == LOCK and locked is not None:
            hit |= locked
            continue
        found = owners(path, everything - set(PACKAGES))
        if found is None:
            return dict.fromkeys(sorted(everything), True)
        hit |= found
    while grown := {name for name, deps in graph.items() if deps & hit} - hit:
        hit |= grown
    hit |= {name for path in changed for name in data(path)}
    return {name: name in hit for name in sorted(everything)}


LOCK = "rust/Cargo.lock"


def locked_crates(before: str, after: str) -> set[str]:
    """The directories of the workspace crates whose locked dependencies differ between two
    texts of Cargo.lock: a package added, removed or at another version or source."""
    old, new = (
        {p["name"]: p for p in tomllib.loads(text).get("package", [])} for text in (before, after)
    )
    # A lock entry names a dependency as `name` or, when two versions are locked, `name version`.
    uses: dict[str, set[str]] = {}
    for lock in (old, new):
        for name, package in lock.items():
            deps = {dep.split()[0] for dep in package.get("dependencies", [])}
            uses.setdefault(name, set()).update(deps)

    def pinned(package: dict[str, object]) -> dict[str, object]:
        return {key: value for key, value in package.items() if key != "dependencies"}

    changed = {
        name
        for name in old.keys() | new.keys()
        if pinned(old.get(name, {})) != pinned(new.get(name, {}))
    }
    found = set()
    for name, (package, _) in workspace().items():
        seen, todo = set(), [package]
        while todo:
            for dep in uses.get(todo.pop(), set()) - seen:
                seen.add(dep)
                todo.append(dep)
        if seen & changed:
            found.add(name)
    return found


def changed_files(base: str) -> list[str]:
    diff = ["git", "diff", "--name-only", "--no-renames", f"{base}...HEAD"]
    return subprocess.run(diff, cwd=ROOT, check=True, capture_output=True, text=True).stdout.split()


def main(argv: list[str]) -> None:
    changed = changed_files(argv[0]) if argv else None
    locked = None
    if changed and LOCK in changed:
        show = ["git", "show", f"{argv[0]}:{LOCK}"]
        before = subprocess.run(show, cwd=ROOT, capture_output=True, text=True)
        if before.returncode == 0:
            locked = locked_crates(before.stdout, (ROOT / LOCK).read_text())
    result = affected(changed, locked)
    crates = [
        (name, package)
        for name, (package, _) in sorted(workspace().items())
        # The extension's own tests run with the Python package's.
        if result[name] and name != "sqlty-ext"
    ]
    lines = [
        f"crates={' '.join(name for name, _ in crates)}",
        f"packages={' '.join(package for _, package in crates)}",
    ]
    lines += [f"{name}={str(result[name]).lower()}" for name in PACKAGES]
    output = os.environ.get("GITHUB_OUTPUT")
    with open(output, "a") if output else sys.stdout as out:
        out.write("".join(f"{line}\n" for line in lines))


if __name__ == "__main__":
    main(sys.argv[1:])
