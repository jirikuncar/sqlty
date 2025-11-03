"""Stub file merging and incremental update utilities.

This module handles merging new SQL query overloads into existing stub files,
supporting both incremental (add only new queries) and full (regenerate class)
modes.
"""

import ast
import logging
from pathlib import Path

from ._collector import CallInfo


class StubMerger:
    """Merge new query overloads into existing stub files.

    Supports:
    - Incremental mode: Add only new overloads
    - Full mode: Regenerate class while preserving imports/aliases
    """

    def __init__(self, function_name: str = "sql"):
        """Initialize the merger.

        Args:
            function_name: Name of the SQL function to generate overloads for
        """
        self.function_name = function_name

    def parse_existing_stub(self, stub_path: Path) -> set[str]:
        """Parse an existing stub file to extract all query literals (any class).

        Note: Kept for backward compatibility. Prefer using
        `parse_stub_class_literals` when you know the class name.
        """
        if not stub_path.exists():
            return set()
        try:
            with open(stub_path) as f:
                stub_content = f.read()
            tree = ast.parse(stub_content)
            return self.collect_literal_strings(tree)
        except Exception as e:
            logging.getLogger(__name__).warning(
                "Failed to parse existing stub %s: %s", stub_path, e
            )
            return set()

    def collect_literal_strings(self, tree: ast.AST) -> set[str]:
        """Collect all string values used inside Literal[...] type annotations."""
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
        except Exception:
            return set()
        for node in tree.body:
            if isinstance(node, ast.ClassDef) and node.name == class_name:
                # Build a temporary module from the class body to reuse collector
                fake_module = ast.Module(body=node.body, type_ignores=[])
                return self.collect_literal_strings(fake_module)
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

        # Generate source location comment using module name derived from file path
        src_comment = ""
        lineno = call.get("lineno")
        file_path = call.get("file")

        if lineno and file_path:
            # Always derive module name from file path for source location
            from pathlib import Path as _P

            module_name = _P(file_path).stem
            src_comment = f"        # from {module_name}:{lineno}"

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
        if src_comment:
            lines.append(src_comment)
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
        """Merge new calls with existing stub file using AST to update only the class.

        - Incremental: add only missing overloads into the class body.
        - Full mode: regenerate the class block; keep imports and aliases intact.
        - Prune: when combined with full_mode, removes overloads not in new_calls.
        """
        # If file doesn't exist, generate a fresh full stub
        if not output_path.exists():
            header = [
                "from typing import overload, Literal",
                f"from sqlty import SQL, {base_class}",
                "",
                "",
            ]
            class_block = self.generate_class_block(
                new_calls, class_name, base_class, relative_to=output_path.parent
            )
            alias = [
                "",
                "# Type alias for the sql function",
                f"{export_name} = {class_name}.{self.function_name}",
                "",
            ]
            return "\n".join(header + [class_block] + alias)

        # Read existing content
        stub_source = output_path.read_text()

        if full_mode:
            # Replace only the class block
            span = self.find_class_span(stub_source, class_name)
            class_block = self.generate_class_block(
                new_calls, class_name, base_class, relative_to=output_path.parent
            )
            if not span:
                # No class found; append at end preserving file
                return stub_source.rstrip() + "\n\n" + class_block + "\n"
            start, end = span
            lines = stub_source.splitlines()
            new_lines = lines[: start - 1] + class_block.splitlines() + lines[end:]
            return "\n".join(new_lines) + ("\n" if stub_source.endswith("\n") else "")

        # Incremental: add only missing overloads
        existing_literals = self.parse_stub_class_literals(stub_source, class_name)
        missing_calls = [c for c in new_calls if c["literal"] not in existing_literals]
        if not missing_calls:
            return stub_source
        insertion_line = self.find_class_insertion_line(stub_source, class_name)
        if insertion_line is None:
            # Fallback: append full class block and alias at end
            class_block = self.generate_class_block(
                new_calls, class_name, base_class, relative_to=output_path.parent
            )
            alias = [
                "",
                "# Type alias for the sql function",
                f"{export_name} = {class_name}.{self.function_name}",
                "",
            ]
            return stub_source.rstrip() + "\n\n" + class_block + "\n".join(alias)
        # Build insertion text
        insertion_text = "".join(
            self.generate_overload_method_block(c, relative_to=output_path.parent) + "\n"
            for c in missing_calls
        )
        lines = stub_source.splitlines()
        # insertion_line is 1-based; insert before this line
        zero_idx = max(0, insertion_line - 1)
        new_lines = lines[:zero_idx] + insertion_text.splitlines() + lines[zero_idx:]
        return "\n".join(new_lines) + ("\n" if stub_source.endswith("\n") else "")
