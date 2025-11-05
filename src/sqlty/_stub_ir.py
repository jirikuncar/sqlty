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
import re
from dataclasses import dataclass, field
from pathlib import Path


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
    """

    query_literal: str
    return_type: str
    source_file: str | None = None
    source_line: int | None = None
    source_span: SourceSpan | None = None
    ast_node: ast.FunctionDef | None = None

    def __hash__(self) -> int:
        """Hash based on query literal for deduplication."""
        return hash(self.query_literal)

    def __eq__(self, other: object) -> bool:
        """Equality based on query literal."""
        if not isinstance(other, OverloadSignature):
            return NotImplemented
        return self.query_literal == other.query_literal


@dataclass
class RegistryClass:
    """Represents a SQLRegistry subclass with its overloads.

    Attributes:
        name: Class name (e.g., "UserQueries")
        base_class: Base class name (e.g., "SQLRegistry")
        overloads: Set of overload signatures for this registry
        export_name: Name used for the module-level alias (e.g., "sql", "user_sql")
        docstring: Optional class docstring
        ast_node: Optional reference to original AST ClassDef node from stub
    """

    name: str
    base_class: str = "SQLRegistry"
    overloads: set[OverloadSignature] = field(default_factory=set)
    export_name: str = "sql"
    docstring: str = "Application-specific SQL query registry with type-safe overloads."
    ast_node: ast.ClassDef | None = None

    def add_overload(self, overload: OverloadSignature) -> bool:
        """Add an overload to this registry.

        Returns:
            True if the overload was added (new), False if it already existed
        """
        initial_size = len(self.overloads)
        self.overloads.add(overload)
        return len(self.overloads) > initial_size

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
    """Represents the complete structure of a .pyi stub file.

    Attributes:
        imports: Set of import statements
        registries: Dictionary mapping class name to RegistryClass
        file_path: Optional path to the stub file
        ast_module: Optional reference to original AST Module node from stub
    """

    imports: set[ImportStatement] = field(default_factory=set)
    registries: dict[str, RegistryClass] = field(default_factory=dict)
    file_path: Path | None = None
    ast_module: ast.Module | None = None

    def add_import(self, import_stmt: ImportStatement) -> None:
        """Add an import statement (automatically deduplicated)."""
        self.imports.add(import_stmt)

    def add_registry(self, registry: RegistryClass) -> None:
        """Add or replace a registry class."""
        self.registries[registry.name] = registry

    def get_registry(self, name: str) -> RegistryClass | None:
        """Get a registry by name."""
        return self.registries.get(name)

    def merge_registry(self, registry: RegistryClass, mode: str = "incremental") -> None:
        """Merge a registry into this stub file.

        Args:
            registry: The registry to merge
            mode: Merge mode - "incremental" or "prune"
                - incremental: Add new overloads, keep existing ones (never removes)
                - prune: Remove overloads not in the new registry (only removes, never adds)
        """
        if mode == "incremental":
            if registry.name in self.registries:
                existing = self.registries[registry.name]
                for overload in registry.overloads:
                    existing.add_overload(overload)
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

        base_classes = {r.base_class for r in self.registries.values()}
        sqlty_imports = ["SQL"] + sorted(base_classes)
        required.add(ImportStatement(module="sqlty", names=sqlty_imports, from_import=True))

        type_patterns = {
            r"\buuid\.UUID\b": ImportStatement(module="uuid", names=[], from_import=False),
            r"\bdatetime\.(datetime|date|time|timedelta)\b": ImportStatement(
                module="datetime", names=[], from_import=False
            ),
            r"\bdecimal\.Decimal\b": ImportStatement(module="decimal", names=[], from_import=False),
        }

        for registry in self.registries.values():
            for overload in registry.overloads:
                return_type = overload.return_type
                for pattern, import_stmt in type_patterns.items():
                    if re.search(pattern, return_type):
                        required.add(import_stmt)

        return required

    def has_same_overloads(self, other: "StubFile") -> bool:
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

            # Compare overload signatures (query literals and return types)
            self_overloads = {(o.query_literal, o.return_type) for o in self_registry.overloads}
            other_overloads = {(o.query_literal, o.return_type) for o in other_registry.overloads}

            if self_overloads != other_overloads:
                return False

        return True

    def compute_overload_diff(
        self, other: "StubFile"
    ) -> dict[str, dict[str, list[tuple[str, str]]]]:
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
                case (None, RegistryClass(overloads=other_overloads)):
                    # Registry exists in other but not in self (new registry)
                    extra = [(o.query_literal, o.return_type) for o in other_overloads]
                case (RegistryClass(overloads=self_overloads), None):
                    # Registry exists in self but not in other (removed registry)
                    missing = [(o.query_literal, o.return_type) for o in self_overloads]
                case (
                    RegistryClass(overloads=self_overloads),
                    RegistryClass(overloads=other_overloads),
                ):
                    # Both registries exist, compare overloads
                    unique_self_overloads: set[tuple[str, str]] = {
                        (o.query_literal, o.return_type) for o in self_overloads
                    }
                    unique_other_overloads: set[tuple[str, str]] = {
                        (o.query_literal, o.return_type) for o in other_overloads
                    }

                    missing = sorted(unique_self_overloads - unique_other_overloads)
                    extra = sorted(unique_other_overloads - unique_self_overloads)

            if missing or extra:
                diff_result[name] = {"missing": missing, "extra": extra}

        return diff_result
