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
class ClassDefinition:
    """Represents a class definition for non-tuple return types.

    Attributes:
        name: The class name (e.g., "QueryRow_abc123")
        format_type: The format type - "typeddict", "namedtuple", or "pydantic"
        fields: Dict mapping field names to their types (e.g., {"id": "int", "name": "str"})
        source_query: The SQL query that generated this class
    """

    name: str
    format_type: str  # "typeddict", "namedtuple", or "pydantic"
    fields: dict[str, str] = field(default_factory=dict)
    source_query: str = ""

    def __hash__(self) -> int:
        """Hash for deduplication."""
        return hash((self.name, self.format_type, tuple(sorted(self.fields.items()))))

    def __eq__(self, other: object) -> bool:
        """Equality for deduplication."""
        if not isinstance(other, ClassDefinition):
            return NotImplemented
        return (
            self.name == other.name
            and self.format_type == other.format_type
            and self.fields == other.fields
        )


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

    def __hash__(self) -> int:
        """Hash based on query literal for deduplication."""
        return hash(self.query_literal)

    def __eq__(self, other: object) -> bool:
        """Equality based on query literal."""
        if not isinstance(other, OverloadSignature):
            return NotImplemented
        return self.query_literal == other.query_literal


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
    """

    name: str
    overloads: set[OverloadSignature] = field(default_factory=set)
    export_name: str = "sql"
    docstring: str = "Application-specific SQL query registry with type-safe overloads."
    ast_node: ast.FunctionDef | None = None
    is_function: bool = False
    custom_imports: list[str] = field(default_factory=list)

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
    """Represents a .pyi stub file with @sqlty decorated functions.

    Attributes:
        registries: Dict of registry name to RegistryDecorator for both classes and functions
        imports: Set of import statements needed for the stub
        class_definitions: Set of class definitions for non-tuple return types
        source_path: Optional path to the source .py file this stub corresponds to
        ast_module: Optional reference to the original AST Module node
    """

    registries: dict[str, RegistryDecorator] = field(default_factory=dict)
    imports: set[ImportStatement] = field(default_factory=set)
    class_definitions: set[ClassDefinition] = field(default_factory=set)
    source_path: Path | None = None
    ast_module: ast.Module | None = None

    def add_import(self, import_stmt: ImportStatement) -> None:
        """Add an import statement (automatically deduplicated)."""
        self.imports.add(import_stmt)

    def add_class_definition(self, class_def: ClassDefinition) -> None:
        """Add a class definition (automatically deduplicated)."""
        self.class_definitions.add(class_def)

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
        for registry in self.registries.values():
            for overload in registry.overloads:
                return_type = overload.return_type
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

        # Add imports for class definitions
        uses_typeddict = False
        uses_namedtuple = False
        uses_pydantic = False
        for class_def in self.class_definitions:
            if class_def.format_type == "typeddict":
                uses_typeddict = True
            elif class_def.format_type == "namedtuple":
                uses_namedtuple = True
            elif class_def.format_type == "pydantic":
                uses_pydantic = True
            
            # Check for datetime/uuid/decimal in class field types
            for field_type in class_def.fields.values():
                for pattern, import_stmt in type_patterns.items():
                    if re.search(pattern, field_type):
                        required.add(import_stmt)

        if uses_typeddict:
            # Add TypedDict to typing import
            typing_import = None
            for imp in required:
                if imp.module == "typing":
                    typing_import = imp
                    break
            if typing_import:
                names = set(typing_import.names)
                names.add("TypedDict")
                required = {imp for imp in required if imp.module != "typing"}
                required.add(
                    ImportStatement(
                        module="typing", names=sorted(list(names)), from_import=True
                    )
                )
            else:
                required.add(
                    ImportStatement(module="typing", names=["TypedDict"], from_import=True)
                )

        if uses_namedtuple:
            required.add(
                ImportStatement(module="collections", names=["namedtuple"], from_import=True)
            )

        if uses_pydantic:
            required.add(
                ImportStatement(module="pydantic", names=["BaseModel"], from_import=True)
            )

        # Add custom imports from registries
        for registry in self.registries.values():
            for import_str in registry.custom_imports:
                # Parse the import string to create ImportStatement
                parsed_import = self._parse_import_string(import_str)
                if parsed_import is not None:
                    required.add(parsed_import)

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
                case (None, RegistryDecorator(overloads=other_overloads)):
                    # Registry exists in other but not in self (new registry)
                    extra = [(o.query_literal, o.return_type) for o in other_overloads]
                case (RegistryDecorator(overloads=self_overloads), None):
                    # Registry exists in self but not in other (removed registry)
                    missing = [(o.query_literal, o.return_type) for o in self_overloads]
                case (
                    RegistryDecorator(overloads=self_overloads),
                    RegistryDecorator(overloads=other_overloads),
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
