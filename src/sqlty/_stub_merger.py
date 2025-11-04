"""Stub file merging and incremental update utilities.

This module handles merging new SQL query overloads into existing stub files,
supporting both incremental (add only new queries) and full (regenerate class)
modes.

This is a compatibility layer that uses the new IR-based implementation internally.
"""

import ast
import logging
import re
from pathlib import Path

from ._collector import CallInfo
from ._stub_ir import StubFile
from ._stub_parser import CallInfoToIRParser, PyiToIRParser
from ._stub_serializer import IRToTextSerializer


def compute_additional_imports(calls: list[CallInfo]) -> list[str]:
    """Compute extra imports needed from return types.

    Returns module names like ["uuid", "datetime", "decimal"].
    """
    patterns = {
        r"\buuid\.UUID\b": "uuid",
        r"\bdatetime\.(datetime|date|time|timedelta)\b": "datetime",
        r"\bdecimal\.Decimal\b": "decimal",
    }
    needed: set[str] = set()
    for call in calls:
        rt = call.get("return_type") or ""
        for pat, mod in patterns.items():
            if re.search(pat, rt):
                needed.add(mod)
    return sorted(needed)


class StubMerger:
    """Merge new query overloads into existing stub files.

    Supports:
    - Incremental mode: Add only new overloads
    - Full mode: Regenerate class while preserving imports/aliases
    - Prune mode: Remove overloads not present in source

    This class now uses the IR-based implementation internally for better
    structure and maintainability.
    """

    def __init__(self, function_name: str = "sql"):
        """Initialize the merger.

        Args:
            function_name: Name of the SQL function to generate overloads for
        """
        self.function_name = function_name
        self._pyi_parser = PyiToIRParser(function_name)
        self._call_parser = CallInfoToIRParser(function_name)
        self._serializer = IRToTextSerializer(function_name)

    def parse_existing_stub(self, stub_path: Path) -> set[str]:
        """Parse an existing stub file to extract all query literals (any class).

        Note: Kept for backward compatibility. Prefer using
        `parse_stub_class_literals` when you know the class name.
        """
        if not stub_path.exists():
            return set()
        try:
            stub_ir = self._pyi_parser.parse_file(stub_path)
            # Collect all literals from all registries
            literals: set[str] = set()
            for registry in stub_ir.registries.values():
                for overload in registry.overloads:
                    literals.add(overload.query_literal)
            return literals
        except Exception as e:
            logging.getLogger(__name__).warning(
                "Failed to parse existing stub %s: %s", stub_path, e
            )
            return set()

    def collect_literal_strings(self, tree: ast.AST) -> set[str]:
        """Collect all string values used inside Literal[...] type annotations.

        Legacy method for backward compatibility.
        """
        literals: set[str] = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Subscript):
                if isinstance(node.value, ast.Name) and node.value.id == "Literal":
                    # Literal["..."]
                    if isinstance(node.slice, ast.Constant) and isinstance(node.slice.value, str):
                        literals.add(node.slice.value)
                    # Literal[("...", "...")] pattern – not used here but future-proof
                    elif hasattr(node.slice, "elts"):
                        for elt in getattr(node.slice, "elts", []):
                            if isinstance(elt, ast.Constant) and isinstance(elt.value, str):
                                literals.add(elt.value)
        return literals

    def parse_stub_class_literals(self, stub_source: str, class_name: str) -> set[str]:
        """Parse stub source and return literals only from the given class body."""
        try:
            tree = ast.parse(stub_source)
            stub_ir = self._pyi_parser.parse_ast(tree)
            registry = stub_ir.get_registry(class_name)
            if registry:
                return {o.query_literal for o in registry.overloads}
            return set()
        except Exception:
            return set()

    def find_class_span(self, stub_source: str, class_name: str) -> tuple[int, int] | None:
        """Find 1-based (start_line, end_line) span of class in source using AST."""
        try:
            tree = ast.parse(stub_source)
        except Exception:
            return None
        for node in tree.body:
            if isinstance(node, ast.ClassDef) and node.name == class_name:
                # end_lineno is available in Python 3.8+
                end_ln = getattr(node, "end_lineno", None)
                ln = getattr(node, "lineno", None)
                if isinstance(ln, int) and isinstance(end_ln, int):
                    return (ln, end_ln)
        return None

    def find_class_insertion_line(self, stub_source: str, class_name: str) -> int | None:
        """Find the line number inside the class to insert new overloads.

        Returns the 1-based absolute line number in the file before which new
        overloads should be inserted (ideally before the generic fallback).
        """
        try:
            tree = ast.parse(stub_source)
        except Exception:
            return None
        for node in tree.body:
            if isinstance(node, ast.ClassDef) and node.name == class_name:
                # Find the generic fallback def sql[T](...) -> T or, if not found,
                # the end of class.
                fallback_line: int | None = None
                for item in node.body:
                    if isinstance(item, ast.FunctionDef) and item.name == self.function_name:
                        # Check decorators
                        has_overload = any(
                            isinstance(dec, ast.Name) and dec.id == "overload"
                            for dec in item.decorator_list
                        )
                        if has_overload:
                            # Determine if this is the generic fallback: no Literal in params
                            func_literals = self.collect_literal_strings(
                                ast.Module(body=[item], type_ignores=[])
                            )
                            if not func_literals:
                                # Consider this as fallback overload
                                fallback_line = item.lineno
                if fallback_line is not None:
                    return fallback_line
                # If no explicit fallback found, insert at end of class body
                end_ln = getattr(node, "end_lineno", None)
                if isinstance(end_ln, int):
                    return end_ln  # insert before class end
        return None

    def generate_overload_method_block(
        self, call: CallInfo, relative_to: Path | None = None
    ) -> str:
        """Generate a single overload method block (indented 4 spaces)."""
        literal: str = call["literal"]
        return_type: str = call.get("return_type") or "Any"

        if "\n" in literal:
            literal_repr = '"""' + literal + '"""'
        else:
            literal_repr = '"' + literal.replace('"', '\\"') + '"'

        lines: list[str] = []
        lines.append("    @overload")
        lines.append("    @classmethod")
        lines.append(f"    def {self.function_name}(")
        lines.append("        cls,")
        lines.append(f"        query: Literal[{literal_repr}],")
        lines.append(f"    ) -> {return_type}:")
        lines.append("        ...")
        lines.append("    ")
        return "\n".join(lines)

    def generate_class_block(
        self,
        calls: list[CallInfo],
        class_name: str,
        base_class: str,
        relative_to: Path | None = None,
    ) -> str:
        """Generate only the class block (no imports/aliases)."""
        # Deduplicate
        seen_literals: set[str] = set()
        unique_calls: list[CallInfo] = []
        for call in calls:
            if call["literal"] not in seen_literals:
                seen_literals.add(call["literal"])
                unique_calls.append(call)

        # Sort calls by literal string for deterministic output across machines
        unique_calls.sort(key=lambda c: c["literal"])

        parts: list[str] = []
        parts.append(f"class {class_name}({base_class}):")
        parts.append('    """Application-specific SQL query registry with type-safe overloads."""')
        parts.append("    ")
        for call in unique_calls:
            parts.append(self.generate_overload_method_block(call, relative_to))
        parts.append("    @overload")
        parts.append("    @classmethod")
        parts.append(f"    def {self.function_name}[T](cls, query: T) -> T: ...")
        parts.append("")
        return "\n".join(parts)

    def merge_stub_incremental(
        self,
        output_path: Path,
        new_calls: list[CallInfo],
        class_name: str = "Query",
        base_class: str = "SQLRegistry",
        full_mode: bool = False,
        export_name: str = "sql",
        prune: bool = False,
    ) -> str:
        """Merge new calls with existing stub file using IR-based approach.

        - Incremental: add only missing overloads into the class body.
        - Full mode: regenerate the class block; keep imports and aliases intact.
        - Prune: when combined with full_mode, removes overloads not in new_calls.
        """
        # Parse new calls to IR
        new_stub = self._call_parser.parse_calls(
            new_calls, class_name, base_class, export_name
        )

        # If file doesn't exist, generate fresh stub
        if not output_path.exists():
            return self._serializer.serialize_to_text(new_stub, relative_to=output_path.parent)

        # Parse existing stub to IR
        existing_stub = self._pyi_parser.parse_file(output_path)

        # Determine merge mode
        if prune:
            mode = "prune"
        elif full_mode:
            mode = "full"
        else:
            mode = "incremental"

        # Merge each registry from new stub into existing stub
        for registry_name, new_registry in new_stub.registries.items():
            existing_stub.merge_registry(new_registry, mode=mode)

        # Serialize back to text
        return self._serializer.serialize_to_text(existing_stub, relative_to=output_path.parent)
