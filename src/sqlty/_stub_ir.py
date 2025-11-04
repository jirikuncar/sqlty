"""Intermediate Representation (IR) for stub file structure.

This module defines the internal data structures used to represent stub file
content in a way that can be easily manipulated (merge, replace, prune) before
being serialized back to AST/text.

The IR provides a clean separation between:
1. Parsing: Converting pyi/source code to IR
2. Manipulation: Merging, replacing, pruning IR elements
3. Serialization: Converting IR back to AST/text
"""

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class OverloadSignature:
    """Represents a single @overload method signature.

    Attributes:
        query_literal: The SQL query string
        return_type: The return type annotation (e.g., "SQL[tuple[int, str]]")
        source_file: Optional source file where this query appears
        source_line: Optional line number in source file
    """

    query_literal: str
    return_type: str
    source_file: str | None = None
    source_line: int | None = None

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
    """

    name: str
    base_class: str = "SQLRegistry"
    overloads: set[OverloadSignature] = field(default_factory=set)
    export_name: str = "sql"
    docstring: str = "Application-specific SQL query registry with type-safe overloads."

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
    """

    module: str
    names: list[str] = field(default_factory=list)
    from_import: bool = True

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
    """

    imports: set[ImportStatement] = field(default_factory=set)
    registries: dict[str, RegistryClass] = field(default_factory=dict)
    file_path: Path | None = None

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
            mode: Merge mode - "incremental", "full", or "prune"
                - incremental: Add new overloads, keep existing ones
                - full: Replace entire registry with new one
                - prune: Remove overloads not in the new registry (only removes, never adds)
        """
        if mode == "full":
            # Replace entire registry
            self.registries[registry.name] = registry
        elif mode == "incremental":
            # Add new overloads to existing registry
            if registry.name in self.registries:
                existing = self.registries[registry.name]
                for overload in registry.overloads:
                    existing.add_overload(overload)
            else:
                self.registries[registry.name] = registry
        elif mode == "prune":
            # Remove overloads that no longer exist in the source (prune only removes, never adds)
            if registry.name in self.registries:
                existing = self.registries[registry.name]
                new_literals = {o.query_literal for o in registry.overloads}
                # Remove overloads not in new set
                to_remove = [
                    o.query_literal
                    for o in existing.overloads
                    if o.query_literal not in new_literals
                ]
                for literal in to_remove:
                    existing.remove_overload(literal)
            else:
                self.registries[registry.name] = registry
        else:
            raise ValueError(f"Unknown merge mode: {mode}")

    def compute_required_imports(self) -> set[ImportStatement]:
        """Compute the set of imports required based on registry contents.

        Returns:
            Set of ImportStatement objects needed for the stub file
        """
        required: set[ImportStatement] = set()

        # Always need typing imports
        required.add(ImportStatement(module="typing", names=["Literal", "overload"], from_import=True))

        # Always need sqlty imports
        base_classes = {r.base_class for r in self.registries.values()}
        sqlty_imports = ["SQL"] + sorted(base_classes)
        required.add(ImportStatement(module="sqlty", names=sqlty_imports, from_import=True))

        # Check return types for additional imports
        import re

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
