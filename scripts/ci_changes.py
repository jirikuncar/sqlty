"""Which CI packages a change affects, with every package that depends on one of them.

Each Rust crate is a package of its own; its workspace dependencies come from its
Cargo.toml. A file no package owns runs every package, unless it is listed in IGNORED.

    python3 scripts/ci_changes.py [BASE]   # BASE omitted: every package

Writes `crates` (a JSON list) and one `true`/`false` per other package to $GITHUB_OUTPUT,
or prints them.
"""

from __future__ import annotations

import fnmatch
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "rust" / "crates"

# Paths a package owns, and the packages it depends on beyond its own crates.
PACKAGES: dict[str, tuple[list[str], list[str]]] = {
    # The extension is tested through the Python package, which reads most of the repository.
    "python": (
        [
            "src/**",
            "tests/**",
            "docs/**",
            "skills/**",
            "examples/**",
            "scripts/**",
            "benchmarks/**",
            # The links of the guides at the root are checked.
            "*.md",
            ".github/*.md",
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
    # The jobs that build and run the engine itself: a change to the docs alone skips them.
    "engine": (
        [
            "src/**",
            "tests/**",
            "benchmarks/**",
            # The package job checks that the sdist carries them.
            "skills/**",
            "pyproject.toml",
            "uv.lock",
            ".github/workflows/pypi.yml",
        ],
        ["sqlty-ext"],
    ),
    # The desktop test runs the sqlty of the virtual environment.
    "vscode": (["editors/vscode/**", "src/sqlty/**", "npm/wasm/**"], ["sqlty-wasm", "sqlty-ext"]),
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
}

# Files outside a crate's directory that its build or tests read.
CRATE_PATHS = {
    "sqlty-cli": ["skills/**", "src/sqlty/rules/**"],
    "sqlty-sql": ["editors/vscode/syntaxes/**"],
}

# A change here runs every package.
EVERYTHING = [
    ".github/workflows/tests.yml",
    ".github/actions/**",
    "scripts/ci_changes.py",
    "scripts/coverage.sh",
    "scripts/coverage-floors.txt",
    "rust/Cargo.toml",
    "rust/Cargo.lock",
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
    if found or matches(path, IGNORED):
        return found
    return None


def affected(changed: list[str] | None) -> dict[str, bool]:
    graph = {name: deps for name, (_, deps) in workspace().items()}
    graph.update({name: set(deps) for name, (_, deps) in PACKAGES.items()})
    everything = set(graph)
    if changed is None:
        return dict.fromkeys(sorted(everything), True)

    hit: set[str] = set()
    for path in changed:
        found = owners(path, everything - set(PACKAGES))
        if found is None:
            return dict.fromkeys(sorted(everything), True)
        hit |= found
    while grown := {name for name, deps in graph.items() if deps & hit} - hit:
        hit |= grown
    return {name: name in hit for name in sorted(everything)}


def changed_files(base: str) -> list[str]:
    diff = ["git", "diff", "--name-only", "--no-renames", f"{base}...HEAD"]
    return subprocess.run(diff, cwd=ROOT, check=True, capture_output=True, text=True).stdout.split()


def main(argv: list[str]) -> None:
    result = affected(changed_files(argv[0]) if argv else None)
    crates = [
        {"dir": name, "package": package}
        for name, (package, _) in sorted(workspace().items())
        # The extension's own tests run with the Python package's.
        if result[name] and name != "sqlty-ext"
    ]
    lines = [f"crates={json.dumps(crates)}"]
    lines += [f"{name}={str(result[name]).lower()}" for name in PACKAGES]
    output = os.environ.get("GITHUB_OUTPUT")
    with open(output, "a") if output else sys.stdout as out:
        out.write("".join(f"{line}\n" for line in lines))


if __name__ == "__main__":
    main(sys.argv[1:])
