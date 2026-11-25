"""scripts/ci_changes.py: the packages CI runs for a change."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("ci_changes", ROOT / "scripts" / "ci_changes.py")
assert spec is not None and spec.loader is not None
ci_changes: Any = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_changes)

CRATES = sorted(p.name for p in (ROOT / "rust" / "crates").iterdir() if p.is_dir())
EVERYTHING = {*CRATES, *ci_changes.PACKAGES}
# The crates whose tests run the mdtest suite.
MDTEST = {"sqlty-sql", "sqlty-python", "sqlty-format"}
# The database tests, which run the queries of the mdtests and docs.
FIDELITY = {"pg-fidelity", "mysql-fidelity"}
# The jobs that build the extension and run it.
ENGINE = {"free-threaded", *FIDELITY, "bench", "package"}


def hit(*changed: str) -> set[str]:
    return {name for name, affected in ci_changes.affected(list(changed)).items() if affected}


# fmt: off
@pytest.mark.parametrize(("changed", "expected"), [
    (["README.md"], {"docs", "package", *MDTEST}),
    (["docs/guide/x.md"], {"docs", "site", *FIDELITY, *MDTEST}),
    (["docs/assets/playground/app.js"], {"python", "playground", "site"}),
    (["docs/robots.txt"], {"python", "site"}),
    (["overrides/main.html", "zensical.toml", ".github/workflows/docs.yml"], {"site"}),
    ([".github/workflows/docs-preview-cleanup.yml"], {"site"}),
    (["src/sqlty/lexer.py"], {"python", "docs", *ENGINE, "vscode", "site"}),
    ([".github/workflows/vscode.yml"], {"vscode"}),
    (["scripts/compat/run.sh"], {"python", "compat"}),
    ([".github/workflows/compat.yml"], {"compat"}),
    (["rust/mdtest/harness.rs"], MDTEST),
    (["plans/2026-11-19-x.md", ".github/workflows/fuzz.yml"], set()),
    (["CONTRIBUTING.md"], {"docs"}),
    ([".github/pull_request_template.md"], {"docs"}),
    (["tests/test_docs.py"], {"python", "docs"}),
    (["skills/sqlty/SKILL.md"], {"sqlty-cli", "sqlty-ext", "python", "docs", *ENGINE, "vscode", "site", *MDTEST}),
    (["npm/wasm/engine.js"], {"playground", "vscode", "site"}),
    (["editors/vscode/src/extension.ts"], {"vscode"}),
    (["src/sqlty/__init__.py"], {"python", "docs", *ENGINE, "vscode", "site"}),
    (["Makefile"], {"python", "bench", "playground", "site"}),
    (["tests/test_cli.py"], {"python"}),
    (["tests/test_threads.py"], {"python", "free-threaded"}),
    (["tests/test_pg_describe.py"], {"python", "pg-fidelity"}),
    (["tests/test_server_describe.py"], {"python", "mysql-fidelity"}),
    (["tests/fixtures/driver_values.toml"], {"python", *FIDELITY}),
    (["tests/mdtest/check/x.md"], {"python", *FIDELITY, *MDTEST}),
    (["tests/conftest.py"], {"python", "docs", "free-threaded", *FIDELITY, "bench"}),
    (["benchmarks/synth.py"], {"python", "bench"}),
    (["tests/check_dist.py"], {"python", "package"}),
    (["rust/crates/sqlty-wasm/src/lib.rs"], {"sqlty-wasm", "vscode", "playground", "site"}),
    (["rust/crates/sqlty-cli/src/lib.rs"], {"sqlty-cli", "sqlty-ext", "python", "docs", *ENGINE, "vscode", "site"}),
    (["rust/crates/sqlty-sql/src/lib.rs"], EVERYTHING - {"sqlty-rules", "compat"}),
    (["rust/Cargo.lock"], EVERYTHING),
    (["somewhere/new.txt"], EVERYTHING),
    (["editors/vscode/syntaxes/sqlty.tmLanguage.json"], EVERYTHING - {"sqlty-rules", "compat"}),
    (["docs/reference/rules/narrow-type.md"], EVERYTHING - {"compat"}),
])
# fmt: on
def test_a_change_runs_its_packages_and_their_dependents(
    changed: list[str], expected: set[str]
) -> None:
    assert hit(*changed) == expected


def test_no_base_runs_every_package() -> None:
    assert set(ci_changes.affected(None)) == EVERYTHING
    assert all(ci_changes.affected(None).values())


def test_the_extension_crate_runs_with_the_python_package(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output = tmp_path / "output"
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    ci_changes.main([])
    lines = dict(line.split("=", 1) for line in output.read_text().splitlines())
    crates = json.loads(lines.pop("crates"))
    assert [crate["dir"] for crate in crates] == [name for name in CRATES if name != "sqlty-ext"]
    assert {"dir": "sqlty-sql", "package": "sqlty-sql"} in crates
    assert lines == dict.fromkeys(ci_changes.PACKAGES, "true")


def test_lint_builds_what_the_hooks_of_the_changed_files_need() -> None:
    """The patterns of the Lint job's `needs` step are the `files` of the hooks of prek.toml
    that need the extension or Node."""
    import tomllib

    import yaml

    hooks = {
        hook["id"]: hook
        for repo in tomllib.loads((ROOT / "prek.toml").read_text())["repos"]
        for hook in repo["hooks"]
    }
    lint = yaml.safe_load((ROOT / ".github" / "workflows" / "tests.yml").read_text())["jobs"]["lint"]
    [step] = [step for step in lint["steps"] if step.get("id") == "needs"]
    assert f"extension=$(has '{hooks['sqlty-check-examples']['files']}')" in step["run"]
    assert f"extension=$(has '{hooks['sqlty-rewrite-examples']['files']}')" in step["run"]
    assert f"node=$(has '{hooks['vscode-check']['files']}')" in step["run"]
