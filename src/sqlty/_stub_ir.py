"""Intermediate Representation (IR) for stub file structure.

This module defines the internal data structures used to represent stub file
content in a way that can be easily manipulated (merge, replace, prune) before
being serialized back to AST/text.

The IR provides a clean separation between:
1. Parsing: Converting pyi/source code to IR
2. Manipulation: Merging, replacing, pruning IR elements
3. Serialization: Converting IR back to AST/text
"""

import ast
import copy
import re
from dataclasses import dataclass, field
from pathlib import Path

UNTYPED_RE = re.compile(r"\bUNTYPED\b")
FALLBACK = "<fallback>"
"""Query shown for the catch-all overload in overload diffs."""


@dataclass
class SourceSpan:
    """Represents precise source location information.

    Attributes:
        start_offset: Starting byte offset in source file
        end_offset: Ending byte offset in source file
        start_line: Starting line number (1-indexed)
        start_col: Starting column number (0-indexed)
        end_line: Ending line number (1-indexed)
        end_col: Ending column number (0-indexed)
    """

    start_offset: int
    end_offset: int
    start_line: int
    start_col: int
    end_line: int
    end_col: int


@dataclass
class OverloadSignature:
    """Represents a single @overload method signature.

    Attributes:
        query_literal: The SQL query string
        return_type: The return type annotation (e.g., "SQL[tuple[int, str]]")
        source_file: Optional source file where this query appears
        source_line: Optional line number in source file
        source_span: Optional precise source location information
        ast_node: Optional reference to original AST FunctionDef node from stub
        function_signature: Optional AST arguments node for full function signature (for configured functions)
        query_param_name: Name of the query parameter (default "query")
        source_return_annotation: Optional AST return annotation from source function
        return_type_template: Optional template for return type with {sqlty} placeholder
        self_type_template: Optional template for self parameter type with {sqlty} placeholder
        return_type_param: Parameter whose argument gave ``return_type``; annotated
            ``type[<return_type>] | None`` in the overload
    """

    query_literal: str
    return_type: str
    source_file: str | None = None
    source_line: int | None = None
    source_span: SourceSpan | None = None
    ast_node: ast.FunctionDef | None = None
    function_signature: ast.arguments | None = None  # Full signature for configured functions
    query_param_name: str = "query"  # Name of the query parameter to replace with Literal
    source_return_annotation: ast.expr | None = None  # Return annotation from source function
    return_type_template: str | None = None  # Template for return type with {sqlty} placeholder
    self_type_template: str | None = (
        None  # Template for self parameter type with {sqlty} placeholder
    )
    return_type_param: str | None = None

    @property
    def annotation(self) -> str:
        """Rendered return annotation, e.g. ``SQL[tuple[int]]`` for a ``tuple[int]`` row.

        Re-rendered on every access; the dataclass is mutable, so the result is not cached.
        """
        return ast.unparse(render_return_annotation(self))

    def __hash__(self) -> int:
        """Hash based on query literal for deduplication."""
        return hash(self.query_literal)

    def __eq__(self, other: object) -> bool:
        """Equality based on query literal."""
        if not isinstance(other, OverloadSignature):
            return NotImplemented
        return self.query_literal == other.query_literal


class _TypeSubstitutor(ast.NodeTransformer):
    """Replaces ``Any`` and ``T`` names with a copy of ``replacement``."""

    def __init__(self, replacement: ast.expr):
        self.replacement = replacement

    def visit_Name(self, node: ast.Name) -> ast.expr:
        return copy.deepcopy(self.replacement) if node.id in ("Any", "T") else node


def render_return_annotation(overload: OverloadSignature) -> ast.expr:
    """Return annotation of an overload: template, then source annotation, then raw type.

    Example:
        >>> o = OverloadSignature("SELECT 1", "tuple[int]", return_type_template="SQL[{sqlty}]")
        >>> ast.unparse(render_return_annotation(o))
        'SQL[tuple[int]]'
    """
    if overload.return_type_template:
        templated = overload.return_type_template.replace("{sqlty}", overload.return_type)
        return ast.parse(templated, mode="eval").body
    if overload.source_return_annotation:
        annotation = copy.deepcopy(overload.source_return_annotation)
        # Keep the source annotation as-is (e.g. list[Any]) when nothing was inferred
        if overload.return_type and overload.return_type != "Any":
            inferred = ast.parse(overload.return_type, mode="eval").body
            result = _TypeSubstitutor(inferred).visit(annotation)
            assert isinstance(result, ast.expr)
            return result
        return annotation
    return ast.parse(overload.return_type, mode="eval").body


@dataclass
class RegistryDecorator:
    """Represents a @sqlty decorated function with its overloads.

    Attributes:
        name: Registry name (class name or function name)
        overloads: Set of function overloads
        export_name: Name used for module-level export
        docstring: Docstring for the registry
        ast_node: Original AST node if available
        is_function: Whether this represents a decorated function vs a class
        custom_imports: List of custom import statements from function config
        fallback_return_type: Return type of the catch-all ``(query: str)`` overload,
            e.g. ``SQL[UNTYPED]``; None keeps the legacy ``str``/``Any`` fallback
    """

    name: str
    overloads: set[OverloadSignature] = field(default_factory=set)
    export_name: str = "sql"
    docstring: str = "Application-specific SQL query registry with type-safe overloads."
    ast_node: ast.FunctionDef | None = None
    is_function: bool = False
    custom_imports: list[str] = field(default_factory=list)
    fallback_return_type: str | None = None

    def add_overload(self, overload: OverloadSignature) -> bool:
        """Add an overload to this registry.

        Returns:
            True if the overload was added (new), False if it already existed
        """
        initial_size = len(self.overloads)
        self.overloads.add(overload)
        return len(self.overloads) > initial_size

    def update(self, other: RegistryDecorator) -> None:
        """Add ``other``'s overloads, replace those whose rendered return annotation changed,
        and union custom imports; never removes.

        Example:
            >>> reg = RegistryDecorator("sql", {OverloadSignature("VALUES (1)", "SQL[Any]")})
            >>> reg.update(RegistryDecorator("sql", {OverloadSignature("VALUES (1)", "SQL[UNTYPED]")}))
            >>> [o.return_type for o in reg.overloads]
            ['SQL[UNTYPED]']
        """
        self.custom_imports = list(dict.fromkeys(self.custom_imports + other.custom_imports))
        current = {o.query_literal: o for o in self.overloads}
        for overload in other.overloads:
            existing = current.get(overload.query_literal)
            if existing is None or existing.annotation != overload.annotation:
                self.overloads.discard(overload)
                self.overloads.add(overload)

    def remove_overload(self, query_literal: str) -> bool:
        """Remove an overload by query literal.

        Returns:
            True if an overload was removed, False if not found
        """
        to_remove = None
        for overload in self.overloads:
            if overload.query_literal == query_literal:
                to_remove = overload
                break
        if to_remove:
            self.overloads.remove(to_remove)
            return True
        return False

    def has_overload(self, query_literal: str) -> bool:
        """Check if an overload exists for a query literal."""
        return any(o.query_literal == query_literal for o in self.overloads)

    def signatures(self) -> set[tuple[str, str]]:
        """(query, return type) pairs, with ``FALLBACK`` for an explicit catch-all."""
        pairs = {(o.query_literal, o.annotation) for o in self.overloads}
        if self.fallback_return_type:
            pairs.add((FALLBACK, self.fallback_return_type))
        return pairs

    def get_sorted_overloads(self) -> list[OverloadSignature]:
        """Get overloads sorted by query literal for deterministic output."""
        return sorted(self.overloads, key=lambda o: o.query_literal)


@dataclass
class ImportStatement:
    """Represents an import statement.

    Attributes:
        module: Module name (e.g., "typing", "uuid")
        names: List of imported names (e.g., ["Literal", "overload"])
        from_import: True if "from X import Y", False if "import X"
        ast_node: Optional reference to original AST Import/ImportFrom node from stub
    """

    module: str
    names: list[str] = field(default_factory=list)
    from_import: bool = True
    ast_node: ast.Import | ast.ImportFrom | None = None

    def __hash__(self) -> int:
        """Hash for deduplication."""
        return hash((self.module, tuple(self.names), self.from_import))

    def __eq__(self, other: object) -> bool:
        """Equality for deduplication."""
        if not isinstance(other, ImportStatement):
            return NotImplemented
        return (
            self.module == other.module
            and self.names == other.names
            and self.from_import == other.from_import
        )


@dataclass
class StubFile:
    """Represents a .pyi stub file with @sqlty decorated functions.

    Attributes:
        registries: Dict of registry name to RegistryDecorator for both classes and functions
        imports: Set of import statements needed for the stub
        source_path: Optional path to the source .py file this stub corresponds to
        ast_module: Optional reference to the original AST Module node
    """

    registries: dict[str, RegistryDecorator] = field(default_factory=dict)
    imports: set[ImportStatement] = field(default_factory=set)
    source_path: Path | None = None
    ast_module: ast.Module | None = None

    def add_import(self, import_stmt: ImportStatement) -> None:
        """Add an import statement (automatically deduplicated)."""
        self.imports.add(import_stmt)

    def add_registry(self, registry: RegistryDecorator) -> None:
        """Add or replace a registry class or function."""
        self.registries[registry.name] = registry

    def get_registry(self, name: str) -> RegistryDecorator | None:
        """Get a registry by name."""
        return self.registries.get(name)

    def merge_registry(self, registry: RegistryDecorator, mode: str = "incremental") -> None:
        """Merge a registry into this stub file.

        Args:
            registry: The registry to merge
            mode: Merge mode - "incremental" or "prune"
                - incremental: Add new overloads, replace those whose rendered return
                  annotation changed (e.g. a stale ``SQL[Any]`` superseded by ``SQL[UNTYPED]``,
                  or a hand-edited return type), and union custom imports; never removes
                - prune: Remove overloads not in the new registry (only removes, never adds)
        """
        if registry.name in self.registries:
            self.registries[registry.name].fallback_return_type = registry.fallback_return_type
        if mode == "incremental":
            if registry.name in self.registries:
                self.registries[registry.name].update(registry)
            else:
                self.registries[registry.name] = registry
        elif mode == "prune":
            if registry.name in self.registries:
                existing = self.registries[registry.name]
                new_literals = {o.query_literal for o in registry.overloads}
                # Remove overloads not in new registry (never adds new ones)
                to_remove = [
                    o.query_literal
                    for o in existing.overloads
                    if o.query_literal not in new_literals
                ]
                for literal in to_remove:
                    existing.remove_overload(literal)
        else:
            raise ValueError(f"Unknown merge mode: {mode}")

    def compute_required_imports(self) -> set[ImportStatement]:
        """Compute the set of imports required based on registry contents.

        Returns:
            Set of ImportStatement objects needed for the stub file
        """
        required: set[ImportStatement] = set()

        required.add(
            ImportStatement(module="typing", names=["Literal", "overload"], from_import=True)
        )

        type_patterns = {
            r"\buuid\.UUID\b": ImportStatement(module="uuid", names=[], from_import=False),
            r"\bdatetime\.(datetime|date|time|timedelta)\b": ImportStatement(
                module="datetime", names=[], from_import=False
            ),
            r"\bdecimal\.Decimal\b": ImportStatement(module="decimal", names=[], from_import=False),
        }

        # Check if Any or SQL is used in any return type or function signature
        uses_any = False
        uses_sql = False
        uses_untyped = False
        for registry in self.registries.values():
            if fallback := registry.fallback_return_type:
                uses_sql = uses_sql or bool(re.search(r"\bSQL\[", fallback))
                uses_untyped = uses_untyped or bool(UNTYPED_RE.search(fallback))
            for overload in registry.overloads:
                return_type = overload.annotation
                # Check for patterns that require imports
                for pattern, import_stmt in type_patterns.items():
                    if re.search(pattern, return_type):
                        required.add(import_stmt)
                # Check if Any is used in return type
                if re.search(r"\bAny\b", return_type):
                    uses_any = True
                # Check if SQL is used in return type
                if re.search(r"\bSQL\[", return_type):
                    uses_sql = True
                if UNTYPED_RE.search(return_type):
                    uses_untyped = True
                # Check if Any is used in function signature
                if overload.function_signature:
                    sig_str = ast.unparse(overload.function_signature)
                    if re.search(r"\bAny\b", sig_str):
                        uses_any = True
                    # Only import Any for non-trivial function signatures
                    # (those with pos-only params or multiple args)
                    sig = overload.function_signature
                    if len(sig.posonlyargs) > 0 or len(sig.args) > 1 or len(sig.kwonlyargs) > 0:
                        uses_any = True

        # Add Any import if needed
        if uses_any:
            # Update the existing typing import to include Any
            typing_import = ImportStatement(
                module="typing", names=["Literal", "overload", "Any"], from_import=True
            )
            # Remove the old one and add the new one with Any
            required = {imp for imp in required if imp.module != "typing"}
            required.add(typing_import)

        # Add SQL import if needed
        if uses_sql:
            required.add(ImportStatement(module="sqlty", names=["SQL"], from_import=True))

        # Add custom imports from registries
        for registry in self.registries.values():
            for import_str in registry.custom_imports:
                # Parse the import string to create ImportStatement
                parsed_import = self._parse_import_string(import_str)
                if parsed_import is not None:
                    required.add(parsed_import)

        if uses_untyped:
            sqlty_imports = {imp for imp in required if imp.from_import and imp.module == "sqlty"}
            names = sorted({name for imp in sqlty_imports for name in imp.names} | {"UNTYPED"})
            required -= sqlty_imports
            required.add(ImportStatement(module="sqlty", names=names, from_import=True))

        return required

    def _parse_import_string(self, import_str: str) -> ImportStatement | None:
        """Parse an import string into an ImportStatement.

        Args:
            import_str: Import string like "from psycopg import Cursor" or "import psycopg"

        Returns:
            ImportStatement or None if parsing fails
        """
        import_str = import_str.strip()

        # Handle "from X import Y, Z" format
        if import_str.startswith("from "):
            parts = import_str[5:].split(" import ", 1)
            if len(parts) == 2:
                module = parts[0].strip()
                names = [name.strip() for name in parts[1].split(",")]
                return ImportStatement(module=module, names=names, from_import=True)

        # Handle "import X" format
        elif import_str.startswith("import "):
            module = import_str[7:].strip()
            return ImportStatement(module=module, names=[], from_import=False)

        return None

    def has_same_overloads(self, other: StubFile) -> bool:
        """Check if two StubFiles have the same overloads per registry.

        Compares only the semantic content (overloads) per registry,
        ignoring metadata like export names, base classes, etc.

        Args:
            other: Another StubFile to compare with

        Returns:
            True if both have the same registries with same overloads
        """
        # Check if registry sets differ
        if set(self.registries.keys()) != set(other.registries.keys()):
            return False

        # Check each registry for differences in overloads only
        for name in self.registries:
            self_registry = self.registries[name]
            other_registry = other.registries[name]

            if self_registry.signatures() != other_registry.signatures():
                return False

        return True

    def compute_overload_diff(self, other: StubFile) -> dict[str, dict[str, list[tuple[str, str]]]]:
        """Compute differences in overloads between two StubFiles.

        Args:
            other: Another StubFile to compare with (expected/new state)

        Returns:
            Dictionary mapping registry names to their differences:
            {
                "RegistryName": {
                    "missing": [(query, return_type), ...],  # In self but not in other
                    "extra": [(query, return_type), ...],     # In other but not in self
                }
            }
        """
        diff_result: dict[str, dict[str, list[tuple[str, str]]]] = {}

        # Get all registry names from both
        all_registry_names = set(self.registries.keys()) | set(other.registries.keys())

        for name in all_registry_names:
            self_registry = self.registries.get(name)
            other_registry = other.registries.get(name)

            missing = []
            extra = []

            match self_registry, other_registry:
                case (None, None):
                    continue  # Should not happen
                case (None, RegistryDecorator() as new):
                    # Registry exists in other but not in self (new registry)
                    extra = sorted(new.signatures())
                case (RegistryDecorator() as old, None):
                    # Registry exists in self but not in other (removed registry)
                    missing = sorted(old.signatures())
                case (RegistryDecorator() as old, RegistryDecorator() as new):
                    missing = sorted(old.signatures() - new.signatures())
                    extra = sorted(new.signatures() - old.signatures())

            if missing or extra:
                diff_result[name] = {"missing": missing, "extra": extra}

        return diff_result
