"""``check`` resolves row types from source: it never executes, evaluates or imports what a source names.

`tests._no_exec` fails any test in which it does; ``tests/no_exec_allowlist.txt`` lists the
functions that still do and may only shrink. See PLAN-NO-EXEC.md.
"""

from __future__ import annotations

import sys
import textwrap

import pytest

from tests import _no_exec


def test_no_new_call_executes_code() -> None:
    assert sorted(_no_exec.sites().keys() - _no_exec.allowed()) == []


def test_allowlist_names_only_calls_that_exist() -> None:
    """Removing the last such call of a function removes its line too."""
    assert sorted(_no_exec.allowed() - _no_exec.sites().keys()) == []


def _sqlty_function(body: str):
    """``body`` compiled as function ``f`` of ``sqlty/core.py``, on lines that import nothing."""
    path = _no_exec.SRC / "core.py"
    imports = _no_exec._import_lines(str(path))
    assert imports is not None
    line = next(n for n in range(2, 10_000) if not {n, n + 1} & imports)
    source = "\n" * (line - 2) + "def f():\n" + textwrap.indent(body, "    ")
    namespace: dict[str, object] = {}
    exec(compile(source, str(path), "exec"), namespace)  # noqa: S102
    return namespace["f"]


@pytest.mark.parametrize(
    ("body", "what"),
    [
        ("exec('x = 1')", "exec of <string>"),
        ("eval('1 + 1')", "exec of <string>"),
        ("__import__('no_exec_marker')", "import no_exec_marker"),
        ("import importlib\nimportlib.import_module('no_exec_marker')", "import no_exec_marker"),
    ],
)
def test_guard_catches(body: str, what: str, tmp_path, monkeypatch) -> None:
    (tmp_path / "no_exec_marker.py").write_text("")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.delitem(sys.modules, "no_exec_marker", raising=False)
    _sqlty_function(body)()
    found, _no_exec._violations[:] = list(_no_exec._violations), []
    assert any(v.startswith("sqlty.core:f ran " + what) for v in found), found
