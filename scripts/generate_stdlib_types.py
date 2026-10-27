"""Write ``src/sqlty/_type_extractor/_stdlib.py``, the standard-library classes sqlty knows.

    uv run python scripts/generate_stdlib_types.py            # rewrite the table
    uv run python scripts/generate_stdlib_types.py --check    # exit 1 if it is out of date

The source is the typeshed that the locked ``mypy`` bundles, read as stubs: nothing of
the standard library is imported. Inferred column types are a closed set
(``info.PYTHON_TYPE_MAP``), so a class is carried with the column types it derives from
only. Left out, and so unknown to ``check``: protocols, classes of the modules that
register column types as virtual subclasses (``numbers``, ``collections.abc``, ...),
private names, and classes no type can derive from the column types of.
``make generate-stdlib-types`` runs it after a ``mypy`` upgrade;
``tests/test_type_extractor.py`` fails until then.
"""

from __future__ import annotations

import argparse
import ast
import sys
from importlib.metadata import version
from pathlib import Path

import mypy

from sqlty._type_extractor.info import PYTHON_TYPE_MAP

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "src" / "sqlty" / "_type_extractor" / "_stdlib.py"
STDLIB = Path(mypy.__file__).parent / "typeshed" / "stdlib"
OPEN_MODULES = frozenset(
    {
        "abc",
        "collections.abc",
        "_collections_abc",
        "numbers",
        "typing",
        "typing_extensions",
        "_typeshed",
    }
)
OPEN_BASES = frozenset({"typing.Protocol", "typing_extensions.Protocol", "abc.ABC"})
COLUMN_TYPES = {
    f"builtins.{name}" if "." not in name else name: name
    for name, tp in PYTHON_TYPE_MAP.items()
    if isinstance(tp, type) and tp is not type(None)
}


class Stubs:
    """The classes and re-exports of the typeshed stubs under ``root``."""

    def __init__(self, root: Path) -> None:
        self.classes: dict[str, ast.ClassDef] = {}
        self.names: dict[str, dict[str, str]] = {}
        self.stars: dict[str, list[str]] = {}
        self.exports: dict[str, set[str]] = {}
        self._ancestors: dict[str, frozenset[str]] = {}
        for path in sorted(root.rglob("*.pyi")):
            parts = path.relative_to(root).with_suffix("").parts
            module = ".".join(parts[:-1] if parts[-1] == "__init__" else parts)
            package = module if parts[-1] == "__init__" else module.rpartition(".")[0]
            self._read(module, package, ast.parse(path.read_text()).body)

    def _read(self, module: str, package: str, body: list[ast.stmt]) -> None:
        names = self.names.setdefault(module, {})
        exports = self.exports.setdefault(module, set())
        for node in body:
            if isinstance(node, ast.If):
                self._read(module, package, node.body + node.orelse)
            elif isinstance(node, ast.ClassDef):
                self.classes[f"{module}.{node.name}"] = node
                names[node.name] = f"{module}.{node.name}"
                exports.add(node.name)
            elif isinstance(node, ast.Import):
                for alias in node.names:
                    if alias.asname:
                        names[alias.asname] = alias.name
                    else:
                        names[alias.name.partition(".")[0]] = alias.name.partition(".")[0]
            elif isinstance(node, ast.ImportFrom):
                base = package.rsplit(".", node.level - 1)[0] if node.level > 1 else package
                source = ".".join(filter(None, [base if node.level else "", node.module or ""]))
                for alias in node.names:
                    if alias.name == "*":
                        self.stars.setdefault(module, []).append(source)
                    else:
                        names.setdefault(alias.asname or alias.name, f"{source}.{alias.name}")
                        if alias.asname == alias.name:
                            exports.add(alias.name)

    def resolve(self, module: str, node: ast.expr) -> str | None:
        """The qualified name of the class ``node`` spells in ``module``."""
        if isinstance(node, ast.Subscript):
            return self.resolve(module, node.value)
        if isinstance(node, ast.Name):
            return self.canonical(self.names.get(module, {}).get(node.id, f"builtins.{node.id}"))
        if isinstance(node, ast.Attribute) and (base := self._dotted(module, node.value)):
            return self.canonical(f"{base}.{node.attr}")
        return None

    def _dotted(self, module: str, node: ast.expr) -> str | None:
        if isinstance(node, ast.Name):
            return self.names.get(module, {}).get(node.id, node.id)
        if isinstance(node, ast.Attribute) and (base := self._dotted(module, node.value)):
            return f"{base}.{node.attr}"
        return None

    def canonical(self, qualname: str, depth: int = 0) -> str | None:
        """Where the class ``qualname`` is defined, following re-exports."""
        if qualname in self.classes:
            return qualname
        module, _, name = qualname.rpartition(".")
        if depth > 16 or module not in self.names:
            return None
        if (target := self.names[module].get(name)) is not None and target != qualname:
            return self.canonical(target, depth + 1)
        for source in self.stars.get(module, []):
            if (found := self.canonical(f"{source}.{name}", depth + 1)) is not None:
                return found
        return None

    def public(self) -> dict[str, str]:
        """Each public ``module.Name`` of a class, by where the class is defined."""
        found: dict[str, str] = {}
        for module in self.names:
            if any(part.startswith("_") for part in module.split(".")):
                continue
            names = set(self.exports[module])
            for source in self.stars.get(module, []):
                names |= {n for n in self.exports.get(source, ()) if not n.startswith("_")}
            for name in sorted(names):
                if not name.startswith("_") and (target := self.canonical(f"{module}.{name}")):
                    found[f"{module}.{name}"] = target
        return found

    def ancestors(self, qualname: str) -> frozenset[str]:
        if (found := self._ancestors.get(qualname)) is None:
            module = qualname.rpartition(".")[0]
            found = frozenset({qualname}).union(
                *(
                    self.ancestors(target)
                    for base in self.classes[qualname].bases
                    if (target := self.resolve(module, base)) not in (None, qualname)
                )
            )
            self._ancestors[qualname] = found
        return found

    def is_open(self, qualname: str) -> bool:
        """Whether ``qualname`` is a protocol or an ABC, which a column type may match unseen."""
        module, node = qualname.rpartition(".")[0], self.classes[qualname]
        bases = [b.value if isinstance(b, ast.Subscript) else b for b in node.bases]
        metaclasses = [k.value for k in node.keywords if k.arg == "metaclass"]
        return bool(
            {self._dotted(module, b) for b in bases} & OPEN_BASES
            or {self._dotted(module, m) for m in metaclasses} & {"abc.ABCMeta"}
        )


def generate() -> str:
    stubs = Stubs(STDLIB)
    columns = {stubs.canonical(q): name for q, name in COLUMN_TYPES.items()}
    rows: list[str] = []
    for public, target in sorted(stubs.public().items()):
        if target in columns or target.rpartition(".")[0] in OPEN_MODULES or stubs.is_open(target):
            continue
        ancestors = stubs.ancestors(target)
        derived = {q for q in ancestors if q in columns}
        minimal = sorted(
            columns[q] for q in derived if not any(q in stubs.ancestors(o) for o in derived - {q})
        )
        try:
            type("_", tuple(PYTHON_TYPE_MAP[name] for name in minimal), {})
        except TypeError:
            continue
        rows.append(f'    "{public}": ({"".join(f'"{name}", ' for name in minimal).rstrip()}),')
    modules = []
    for line in (STDLIB / "VERSIONS").read_text().splitlines():
        if (line := line.partition("#")[0].strip()) and not line.startswith("_"):
            name, _, span = line.partition(": ")
            low, _, high = span.partition("-")
            if tuple(map(int, low.split("."))) > (3, 10) or high:
                bounds = ", ".join(
                    f"({v.replace('.', ', ')})" if v else "None" for v in (low, high)
                )
                modules.append(f'    "{name}": ({bounds}),')
    return f'''"""Standard-library classes by qualified name, with the column types each derives from.

Generated by ``scripts/generate_stdlib_types.py`` from the typeshed of mypy {version("mypy")}; do
not edit. ``MODULES`` holds the Python versions of the modules not in every supported one.
"""

TYPESHED = "mypy {version("mypy")}"

MODULES: dict[str, tuple[tuple[int, int], tuple[int, int] | None]] = {{
{chr(10).join(modules)}
}}

CLASSES: dict[str, tuple[str, ...]] = {{
{chr(10).join(rows)}
}}
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.partition("\n")[0])
    parser.add_argument("--check", action="store_true", help="exit 1 if the table is out of date")
    text = generate()
    if parser.parse_args().check:
        sys.exit(OUTPUT.read_text() != text)
    OUTPUT.write_text(text)


if __name__ == "__main__":
    main()
