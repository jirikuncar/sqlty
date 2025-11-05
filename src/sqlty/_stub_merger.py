"""Stub file merging and incremental update utilities.

Compatibility layer using IR-based implementation internally.
"""

import re
from pathlib import Path

from ._collector import CallInfo
from ._stub_parser import CallInfoToIRParser, PyiToIRParser
from ._stub_serializer import IRToTextSerializer


def compute_additional_imports(calls: list[CallInfo]) -> list[str]:
    """Compute extra imports needed from return types.

    Legacy function kept for backward compatibility with _generator.py.
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
    """Merge new query overloads into existing stub files using IR-based approach."""

    def __init__(self, function_name: str = "sql"):
        self.function_name = function_name
        self._pyi_parser = PyiToIRParser(function_name)
        self._call_parser = CallInfoToIRParser(function_name)
        self._serializer = IRToTextSerializer(function_name)

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
        """Merge new calls with existing stub file.

        Args:
            output_path: Path to stub file
            new_calls: SQL calls to add
            class_name: Registry class name
            base_class: Base class name
            full_mode: Replace entire registry
            export_name: Exported function name
            prune: Remove stale overloads

        Returns:
            Generated stub content
        """
        new_stub = self._call_parser.parse_calls(new_calls, class_name, base_class, export_name)

        if not output_path.exists():
            return self._serializer.serialize_to_text(new_stub, relative_to=output_path.parent)

        # Read original source and parse existing stub
        original_source = output_path.read_text(encoding="utf-8")
        existing_stub = self._pyi_parser.parse_file(output_path)

        if full_mode:
            # Full replace: ignore existing, serialize new stub without preservation
            return self._serializer.serialize_to_text(
                new_stub, relative_to=output_path.parent, preserve_structure=False
            )

        # Apply incremental and optionally prune to existing stub
        for new_registry in new_stub.registries.values():
            # Add new overloads
            existing_stub.merge_registry(new_registry, mode="incremental")
            if prune:
                # Remove stale overloads
                existing_stub.merge_registry(new_registry, mode="prune")

        # Use surgical editing to preserve comments and structure
        # Reuse existing_stub which already has source spans from line 73
        return self._serializer.serialize_to_text(
            existing_stub,
            relative_to=output_path.parent,
            preserve_structure=True,
            original_source=original_source,
            original_stub=existing_stub,
        )
