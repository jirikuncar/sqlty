"""Stub file merging and incremental update utilities.

Compatibility layer using IR-based implementation internally.
"""

import re
from pathlib import Path
from typing import TYPE_CHECKING

from ._collector import CallInfo
from ._stub_ir import StubFile
from ._stub_parser import CallInfoToIRParser, PyiToIRParser
from ._stub_serializer import IRToTextSerializer

if TYPE_CHECKING:
    from ._config import FunctionConfigComplete
    from ._registry import RegistryDetector


def compute_additional_imports(calls: list[CallInfo]) -> list[str]:
    """Compute extra imports needed from return types.

    Legacy function kept for backward compatibility with _generator.py.
    """
    patterns = {
        r"\buuid\.UUID\b": "uuid",
        r"\bdatetime\.(datetime|date|time|timedelta)\b": "datetime",
        r"\bdecimal\.Decimal\b": "decimal",
        r"\btyping\.Literal\b": "typing",
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

    def __init__(
        self,
        function_name: str = "sql",
        untyped: bool = False,
        registry_detector: RegistryDetector | None = None,
        function_configs: list[FunctionConfigComplete] | None = None,
    ):
        self.function_name = function_name
        self._pyi_parser = PyiToIRParser(function_name)
        self._call_parser = CallInfoToIRParser(
            function_name,
            registry_detector=registry_detector,
            function_configs=function_configs,
            untyped=untyped,
        )
        self._serializer = IRToTextSerializer(function_name)

    def merge_stub_incremental(
        self,
        output_path: Path,
        new_calls: list[CallInfo],
        class_name: str = "Query",
        full_mode: bool = False,
        export_name: str = "sql",
        prune: bool = False,
    ) -> str:
        """Merge new calls with existing stub file.

        Args:
            output_path: Path to stub file
            new_calls: SQL calls to add
            class_name: Registry class name
            full_mode: Replace entire registry
            export_name: Exported function name
            prune: Remove stale overloads

        Returns:
            Generated stub content
        """
        new_stub = self._call_parser.parse_calls(new_calls, class_name, export_name)

        if not output_path.exists():
            return self._serializer.serialize_to_text(new_stub, relative_to=output_path.parent)

        # Read original source and parse existing stub
        original_stub = self._pyi_parser.parse_file(output_path)

        if full_mode:
            # Full replace: ignore existing, serialize new stub without preservation
            return self._serializer.serialize_to_text(
                new_stub, relative_to=output_path.parent, preserve_structure=False
            )

        # Create merged stub by copying original and applying changes
        merged_stub = StubFile(source_path=output_path)
        # Copy imports
        for import_stmt in original_stub.imports:
            merged_stub.add_import(import_stmt)
        # Copy registries
        for _reg_name, reg in original_stub.registries.items():
            merged_stub.add_registry(reg)

        # Apply incremental and optionally prune to merged stub
        for new_registry in new_stub.registries.values():
            # Add new overloads
            merged_stub.merge_registry(new_registry, mode="incremental")
            if prune:
                # Remove stale overloads
                merged_stub.merge_registry(new_registry, mode="prune")

        # Use surgical editing to preserve comments and structure
        # For incremental merges, regenerate from scratch to ensure correctness
        return self._serializer.serialize_to_text(
            merged_stub,
            relative_to=output_path.parent,
            preserve_structure=False,  # Regenerate from scratch for incremental merges
        )
