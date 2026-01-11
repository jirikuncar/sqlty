"""Serializers for converting IR to AST and text.

This module provides serializers to convert the intermediate representation
back to AST nodes and formatted Python code.
"""

import ast
from collections import Counter
from pathlib import Path
from typing import Literal

from ._stub_ir import ImportStatement, OverloadSignature, RegistryDecorator, StubFile


class IRToASTSerializer:
    """Serializes intermediate representation to AST."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the serializer.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
        """
        self.function_name = function_name

    def serialize_to_module(self, stub: StubFile) -> ast.Module:
        """Serialize a StubFile to an AST Module.

        Args:
            stub: StubFile IR to serialize

        Returns:
            AST Module node
        """
        module_body: list[ast.stmt] = []

        imports = self._sort_imports(stub.compute_required_imports())
        for import_stmt in imports:
            module_body.append(self._serialize_import(import_stmt))

        # Add class definitions for non-tuple return types
        for class_def in sorted(stub.class_definitions, key=lambda c: c.name):
            class_node = self._serialize_class_definition(class_def)
            if class_node:
                module_body.append(class_node)

        # Emit top-level function overloads for all decorated functions
        for registry_name in sorted(stub.registries.keys()):
            registry = stub.registries[registry_name]
            function_overloads = self._serialize_function_overloads(registry)
            module_body.extend(function_overloads)

        module = ast.Module(body=module_body, type_ignores=[])
        ast.fix_missing_locations(module)
        return module

    def _count_export_names(self, stub: StubFile) -> Counter[str]:
        """Count how many registries use each export name.

        Args:
            stub: StubFile to analyze

        Returns:
            Counter mapping export names to their usage count
        """
        return Counter(
            registry.export_name
            for registry in stub.registries.values()
            if registry.export_name and not registry.is_function
        )

    def _sort_imports(self, imports: set[ImportStatement]) -> list[ImportStatement]:
        """Sort imports in PEP 8 order.

        Order:
        1. Standard library imports (non-from)
        2. Standard library from imports (typing)
        3. Third-party/local imports (sqlty)

        Args:
            imports: Set of import statements

        Returns:
            Sorted list of imports
        """
        stdlib_modules = {"uuid", "datetime", "decimal"}
        typing_modules = {"typing"}

        stdlib_imports = []
        typing_imports = []
        other_imports = []

        for imp in imports:
            if imp.module in stdlib_modules and not imp.from_import:
                stdlib_imports.append(imp)
            elif imp.module in typing_modules:
                typing_imports.append(imp)
            else:
                other_imports.append(imp)

        # Sort each group by module name
        stdlib_imports.sort(key=lambda x: x.module)
        typing_imports.sort(key=lambda x: x.module)
        other_imports.sort(key=lambda x: x.module)

        return stdlib_imports + typing_imports + other_imports

    def _serialize_import(self, import_stmt: ImportStatement) -> ast.stmt:
        """Serialize an ImportStatement to AST.

        Args:
            import_stmt: ImportStatement to serialize

        Returns:
            AST Import or ImportFrom node
        """
        if import_stmt.from_import:
            return ast.ImportFrom(
                module=import_stmt.module,
                names=[ast.alias(name=name, asname=None) for name in import_stmt.names],
                level=0,
            )
        else:
            return ast.Import(names=[ast.alias(name=import_stmt.module, asname=None)])

    def _serialize_overload(self, overload: OverloadSignature) -> ast.FunctionDef:
        """Serialize an OverloadSignature to AST FunctionDef.

        Args:
            overload: OverloadSignature to serialize

        Returns:
            AST FunctionDef node
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        literal_annotation = ast.Subscript(
            value=ast.Name(id="Literal", ctx=ast.Load()),
            slice=ast.Constant(value=overload.query_literal),
            ctx=ast.Load(),
        )

        query_arg = ast.arg(arg="query", annotation=literal_annotation)
        cls_arg = ast.arg(arg="cls", annotation=None)

        return_annotation = ast.parse(overload.return_type, mode="eval").body

        return ast.FunctionDef(
            name=self.function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[cls_arg, query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=return_annotation,
            type_params=[],
        )

    def _serialize_fallback(self) -> ast.FunctionDef:
        """Serialize the generic fallback overload.

        Returns:
            AST FunctionDef node for the fallback
        """
        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
            ast.Name(id="classmethod", ctx=ast.Load()),
        ]

        type_param = ast.TypeVar(name="T")

        cls_arg = ast.arg(arg="cls", annotation=None)
        query_arg = ast.arg(arg="query", annotation=ast.Name(id="T", ctx=ast.Load()))

        return ast.FunctionDef(
            name=self.function_name,
            args=ast.arguments(
                posonlyargs=[],
                args=[cls_arg, query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            ),
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=ast.Name(id="T", ctx=ast.Load()),
            type_params=[type_param],
        )

    def _serialize_class_definition(self, class_def: "ClassDefinition") -> ast.stmt | None:
        """Serialize a class definition to an AST node.

        Args:
            class_def: ClassDefinition to serialize

        Returns:
            AST ClassDef or Assign node
        """
        from ._stub_ir import ClassDefinition

        if class_def.format_type == "typeddict":
            # Generate: class ClassName(TypedDict):
            #               field1: type1
            #               field2: type2
            class_body: list[ast.stmt] = []
            for field_name, field_type in sorted(class_def.fields.items()):
                # Create annotation: field_name: field_type
                ann_assign = ast.AnnAssign(
                    target=ast.Name(id=field_name, ctx=ast.Store()),
                    annotation=ast.Name(id=field_type, ctx=ast.Load()),
                    simple=1,
                )
                class_body.append(ann_assign)

            if not class_body:
                class_body.append(ast.Pass())

            return ast.ClassDef(
                name=class_def.name,
                bases=[ast.Name(id="TypedDict", ctx=ast.Load())],
                keywords=[],
                body=class_body,
                decorator_list=[],
            )

        elif class_def.format_type == "namedtuple":
            # Generate: ClassName = namedtuple('ClassName', ['field1', 'field2'])
            field_names = list(class_def.fields.keys())
            return ast.Assign(
                targets=[ast.Name(id=class_def.name, ctx=ast.Store())],
                value=ast.Call(
                    func=ast.Name(id="namedtuple", ctx=ast.Load()),
                    args=[
                        ast.Constant(value=class_def.name),
                        ast.List(
                            elts=[ast.Constant(value=name) for name in field_names],
                            ctx=ast.Load(),
                        ),
                    ],
                    keywords=[],
                ),
            )

        elif class_def.format_type == "pydantic":
            # Generate: class ClassName(BaseModel):
            #               field1: type1
            #               field2: type2
            class_body: list[ast.stmt] = []
            for field_name, field_type in sorted(class_def.fields.items()):
                ann_assign = ast.AnnAssign(
                    target=ast.Name(id=field_name, ctx=ast.Store()),
                    annotation=ast.Name(id=field_type, ctx=ast.Load()),
                    simple=1,
                )
                class_body.append(ann_assign)

            if not class_body:
                class_body.append(ast.Pass())

            return ast.ClassDef(
                name=class_def.name,
                bases=[ast.Name(id="BaseModel", ctx=ast.Load())],
                keywords=[],
                body=class_body,
                decorator_list=[],
            )

        return None

    def _serialize_function_overloads(self, registry: RegistryDecorator) -> list[ast.stmt]:
        """Serialize function overloads for @sqlty decorated functions.

        Args:
            registry: RegistryDecorator representing a decorated function

        Returns:
            List of AST FunctionDef nodes (overloads + fallback)
        """
        function_overloads: list[ast.stmt] = []

        # Check if any overload has a custom function signature (configured function)
        # Only treat it as custom if it's non-trivial (has pos-only params or more than just query param)
        overload_list = registry.get_sorted_overloads()
        template_sig = None

        if overload_list and overload_list[0].function_signature:
            sig = overload_list[0].function_signature
            # Check if signature is non-trivial:
            # - Has positional-only parameters, OR
            # - Has more than one regular argument, OR
            # - Has keyword-only arguments
            if len(sig.posonlyargs) > 0 or len(sig.args) > 1 or len(sig.kwonlyargs) > 0:
                template_sig = sig

        for overload in overload_list:
            func_node = self._serialize_function_overload(overload, registry.name)
            function_overloads.append(func_node)

        # Only use template signature if it's non-trivial
        if template_sig:
            fallback = self._serialize_function_fallback(
                registry.name, template_signature=template_sig
            )
        else:
            fallback = self._serialize_function_fallback(registry.name)
        function_overloads.append(fallback)

        return function_overloads

    def _substitute_type_in_annotation(
        self, annotation: ast.expr, replacement: ast.expr
    ) -> ast.expr:
        """Recursively substitute Any or generic type T with replacement type.

        Args:
            annotation: The annotation AST to process
            replacement: The AST node to substitute in place of Any/T

        Returns:
            Modified annotation AST
        """

        class TypeSubstitutor(ast.NodeTransformer):
            def __init__(self, replacement_node: ast.expr):
                self.replacement = replacement_node

            def visit_Name(self, node: ast.Name) -> ast.expr:
                # Replace Any or T with the inferred type
                if node.id in ("Any", "T"):
                    import copy

                    return copy.deepcopy(self.replacement)
                return node

            def visit_Subscript(self, node: ast.Subscript) -> ast.expr:
                # For Union types (e.g., T | None), replace T but keep the union structure
                # Visit the children
                self.generic_visit(node)
                return node

        substitutor = TypeSubstitutor(replacement)
        result = substitutor.visit(annotation)
        assert isinstance(result, ast.expr)
        return result

    def _serialize_function_overload(
        self, overload: OverloadSignature, function_name: str
    ) -> ast.FunctionDef:
        """Serialize a function overload (without classmethod decorator).

        Args:
            overload: OverloadSignature to serialize
            function_name: Name of the function

        Returns:
            AST FunctionDef node
        """
        import copy

        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
        ]

        literal_annotation = ast.Subscript(
            value=ast.Name(id="Literal", ctx=ast.Load()),
            slice=ast.Constant(value=overload.query_literal),
            ctx=ast.Load(),
        )

        # Build return annotation
        # Priority 1: Use return_type_template if provided
        if overload.return_type_template:
            # Replace {sqlty} placeholder with the inferred type
            templated_return_type = overload.return_type_template.replace(
                "{sqlty}", overload.return_type
            )
            return_annotation = ast.parse(templated_return_type, mode="eval").body
        # Priority 2: Use source annotation if available
        elif overload.source_return_annotation:
            # We have a source return annotation from the function definition
            import copy

            return_annotation = copy.deepcopy(overload.source_return_annotation)

            # If we also have an inferred type (not Any), substitute it
            if overload.return_type and overload.return_type != "Any":
                inferred_type_ast = ast.parse(overload.return_type, mode="eval").body
                # Replace Any or generic type parameter T with the inferred type
                return_annotation = self._substitute_type_in_annotation(
                    return_annotation, inferred_type_ast
                )
            # else: keep source annotation as-is (e.g., list[Any] stays list[Any])
        else:
            # Use the inferred return type directly
            return_annotation = ast.parse(overload.return_type, mode="eval").body

        # Check if we have a custom function signature (for configured functions)
        if overload.function_signature:
            # Clone the signature and modify the query parameter
            args = copy.deepcopy(overload.function_signature)

            # Find and replace the query parameter's annotation with Literal
            query_param_name = overload.query_param_name
            param_found = False

            # Also replace return_tp parameter type if present
            # The return_tp should be constrained to type[<inferred_type>]
            return_tp_annotation = None
            if overload.return_type:
                # Extract base type from return type (remove | None suffix if present)
                base_type = overload.return_type.replace(" | None", "").strip()
                # Also handle list[...] and tuple[...] wrappers
                # For list[int], we want type[int], not type[list[int]]
                if base_type.startswith("list[") and base_type.endswith("]"):
                    base_type = base_type[5:-1]  # Extract the inner type
                elif base_type.startswith("tuple[") and base_type.endswith("]"):
                    base_type = base_type[6:-1]  # Extract the inner type

                # Create type[<base_type>] | None annotation
                return_tp_annotation = ast.BinOp(
                    left=ast.Subscript(
                        value=ast.Name(id="type", ctx=ast.Load()),
                        slice=ast.Name(id=base_type, ctx=ast.Load()),
                        ctx=ast.Load(),
                    ),
                    op=ast.BitOr(),
                    right=ast.Constant(value=None),
                )

            # Check in regular args
            for arg in args.args:
                if arg.arg == query_param_name:
                    arg.annotation = literal_annotation
                    param_found = True
                elif arg.arg == "return_tp" and return_tp_annotation:
                    # Replace return_tp type with specific type constraint
                    arg.annotation = return_tp_annotation
                elif arg.arg == "self" and overload.self_type_template:
                    # TODO: self_type_template is not currently supported for methods
                    # Keeping this code for future implementation when method support is added
                    # Apply self type template with {sqlty} substitution
                    templated_self_type = overload.self_type_template.replace(
                        "{sqlty}", overload.return_type
                    )
                    arg.annotation = ast.parse(templated_self_type, mode="eval").body

            # Check in keyword-only args if not found
            if not param_found:
                for arg in args.kwonlyargs:
                    if arg.arg == query_param_name:
                        arg.annotation = literal_annotation
                        param_found = True
                    elif arg.arg == "return_tp" and return_tp_annotation:
                        arg.annotation = return_tp_annotation

            # Check in positional-only args if not found
            if not param_found:
                for arg in args.posonlyargs:
                    if arg.arg == query_param_name:
                        arg.annotation = literal_annotation
                    elif arg.arg == "return_tp" and return_tp_annotation:
                        arg.annotation = return_tp_annotation

            function_args = args
        else:
            # Default: simple query parameter
            query_arg = ast.arg(arg="query", annotation=literal_annotation)
            function_args = ast.arguments(
                posonlyargs=[],
                args=[query_arg],
                kwonlyargs=[],
                kw_defaults=[],
                defaults=[],
            )

        return ast.FunctionDef(
            name=function_name,
            args=function_args,
            body=[ast.Expr(value=ast.Constant(value=...))],
            decorator_list=decorators,
            returns=return_annotation,
            type_params=[],
        )

    def _serialize_function_fallback(
        self, function_name: str, template_signature: ast.arguments | None = None
    ) -> ast.FunctionDef:
        """Serialize the generic fallback overload for a decorated/configured function.

        Args:
            function_name: Name of the function
            template_signature: Optional signature to use as template for configured functions.
                               If provided, uses this as the fallback signature with generic
                               Any annotations for all parameters. If not provided, uses simple
                               (query: str) -> str. Both cases include @overload decorator since
                               this is for .pyi stub files.

        Returns:
            AST FunctionDef node for the fallback with @overload decorator
        """
        import copy

        decorators: list[ast.expr] = [
            ast.Name(id="overload", ctx=ast.Load()),
        ]

        if template_signature:
            # For configured functions, use the template signature but set all annotations to Any
            args = copy.deepcopy(template_signature)

            # Replace all annotations with Any
            any_annotation = ast.Name(id="Any", ctx=ast.Load())
            for arg in args.args:
                arg.annotation = copy.deepcopy(any_annotation)
            for arg in args.posonlyargs:
                arg.annotation = copy.deepcopy(any_annotation)
            for arg in args.kwonlyargs:
                arg.annotation = copy.deepcopy(any_annotation)
            # Note: **params: Any should already have Any annotation, keep it

            # Use Any as the generic fallback return type for configured functions
            return ast.FunctionDef(
                name=function_name,
                args=args,
                body=[ast.Expr(value=ast.Constant(value=...))],
                decorator_list=decorators,
                returns=ast.Name(id="Any", ctx=ast.Load()),
                type_params=[],
            )
        else:
            # Default: simple query parameter
            # This creates: @overload def func(query: str) -> str: ...
            # Which acts as a catch-all for string literals
            query_arg = ast.arg(arg="query", annotation=ast.Name(id="str", ctx=ast.Load()))

            return ast.FunctionDef(
                name=function_name,
                args=ast.arguments(
                    posonlyargs=[],
                    args=[query_arg],
                    kwonlyargs=[],
                    kw_defaults=[],
                    defaults=[],
                ),
                body=[ast.Expr(value=ast.Constant(value=...))],
                decorator_list=decorators,
                returns=ast.Name(id="str", ctx=ast.Load()),
                type_params=[],  # No type params for simpler stub
            )


class IRToTextSerializer:
    """Serializes intermediate representation to formatted Python code."""

    def __init__(self, function_name: str = "sql") -> None:
        """Initialize the serializer.

        Args:
            function_name: Name of the SQL function (e.g., "sql")
        """
        self.function_name = function_name
        self.ast_serializer = IRToASTSerializer(function_name)

    def serialize_to_text(
        self,
        stub: StubFile,
        relative_to: Path | None = None,
        preserve_structure: bool = False,
        original_source: str | None = None,
        original_stub: StubFile | None = None,
    ) -> str:
        """Serialize a StubFile to formatted Python text.

        Args:
            stub: StubFile IR to serialize
            relative_to: Optional path for relative source comments
            preserve_structure: If True and original_source/original_stub are provided,
                               use surgical edits to preserve comments and formatting
            original_source: Optional original source text for surgical editing
            original_stub: Optional original parsed stub for surgical editing

        Returns:
            Formatted Python code as string
        """
        # If we have original source/stub and preserve_structure is requested,
        # use surgical editing to preserve comments
        if preserve_structure and original_source is not None and original_stub is not None:
            return self._serialize_with_preservation(stub, original_source, original_stub)

        # Otherwise, generate from scratch
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _serialize_with_preservation(
        self, stub: StubFile, original_source: str, original_stub: StubFile
    ) -> str:
        """Serialize while preserving original structure and comments.

        This method makes surgical edits based on source spans using
        asttokens.util.replace to preserve everything else.

        Args:
            stub: StubFile with new content to serialize
            original_source: Original source text
            original_stub: Original parsed stub with source spans

        Returns:
            Modified source code with preserved structure
        """
        # Collect all replacements as (start_offset, end_offset, new_text) tuples
        replacements: list[tuple[int, int, str]] = []

        # For each registry in the stub, identify changes
        for registry_name, new_registry in stub.registries.items():
            original_registry = original_stub.registries.get(registry_name)
            if not original_registry:
                # Registry doesn't exist in original - fallback to full regeneration
                return self._fallback_to_full_generation(stub)

            # 1. Identify overloads to remove (in original but not in new stub)
            for orig_overload in original_registry.overloads:
                if not new_registry.has_overload(orig_overload.query_literal):
                    if orig_overload.source_span:
                        # Remove by replacing with empty string
                        # Include any preceding comments and trailing blank lines
                        start_offset = self._find_removal_start_offset(
                            original_source, orig_overload.source_span.start_offset
                        )
                        end_offset = self._find_removal_end_offset(
                            original_source, orig_overload.source_span.end_offset
                        )
                        replacements.append((start_offset, end_offset, ""))

            # 2. Identify overloads to add (in new stub but not in original)
            overloads_to_add = []
            for new_overload in new_registry.overloads:
                if not original_registry.has_overload(new_overload.query_literal):
                    overloads_to_add.append(new_overload)

            # 3. Add new overloads at the appropriate location
            if overloads_to_add:
                insertion_offset, new_text = self._generate_overload_insertions(
                    overloads_to_add, new_registry, original_registry
                )
                if insertion_offset is not None:
                    replacements.append((insertion_offset, insertion_offset, new_text))

        # Apply all replacements using asttokens.util.replace
        if replacements:
            import asttokens.util

            # Sort replacements by start offset (required by asttokens.util.replace)
            replacements.sort(key=lambda r: r[0])
            result = asttokens.util.replace(original_source, replacements)
            return result
        else:
            # No changes needed
            return original_source

    def _find_removal_start_offset(self, source: str, start_offset: int) -> int:
        """Find the actual start offset for removal, including preceding comments.

        Args:
            source: Original source text
            start_offset: Initial start offset from source span

        Returns:
            Adjusted start offset that includes preceding comment lines
        """
        # Find the start of the line containing start_offset
        line_start = start_offset
        while line_start > 0 and source[line_start - 1] != "\n":
            line_start -= 1

        # Look backwards from the line to include any preceding comment lines
        result_offset = line_start
        i = line_start - 1
        while i >= 0:
            # Skip back over the newline
            if source[i] == "\n":
                i -= 1
                if i < 0:
                    break
                # Find the start of this line
                prev_line_start = i
                while prev_line_start > 0 and source[prev_line_start - 1] != "\n":
                    prev_line_start -= 1
                # Check if this line is a comment (after stripping leading whitespace)
                line_content = source[prev_line_start : i + 1].lstrip()
                if line_content.startswith("#"):
                    # This is a comment line - include it
                    result_offset = prev_line_start
                    i = prev_line_start - 1
                else:
                    # Not a comment - stop here
                    break
            else:
                break

        return result_offset

    def _find_removal_end_offset(self, source: str, end_offset: int) -> int:
        """Find the actual end offset for removal, including trailing blank lines.

        Args:
            source: Original source text
            end_offset: Initial end offset from source span

        Returns:
            Adjusted end offset that includes trailing blank lines
        """
        # Look ahead from end_offset to skip any blank lines
        i = end_offset
        while i < len(source):
            # Skip over any consecutive newlines (blank lines)
            if source[i] == "\n":
                i += 1
                continue
            else:
                break
        return i

    def _fallback_to_full_generation(self, stub: StubFile) -> str:
        """Fallback to full stub generation when surgical editing isn't possible."""
        module = self.ast_serializer.serialize_to_module(stub)
        code = ast.unparse(module)
        lines = code.split("\n")
        formatted_lines = self._add_blank_lines(lines)
        return "\n".join(formatted_lines) + "\n"

    def _generate_overload_insertions(
        self,
        overloads: list[OverloadSignature],
        new_registry: RegistryDecorator,
        original_registry: RegistryDecorator,
    ) -> tuple[int | None, str]:
        """Generate text for new overloads and find insertion offset.

        Args:
            overloads: Overloads to add
            new_registry: New registry with overloads
            original_registry: Original registry from parsed file

        Returns:
            Tuple of (insertion_offset, new_text) or (None, "") if no insertion point found
        """
        # Find the insertion point - after the last existing overload but before the fallback
        insertion_offset = None

        # Find the last concrete overload (has source_span and query_literal)
        last_overload = None
        for orig_overload in sorted(
            original_registry.overloads,
            key=lambda o: o.source_span.start_offset if o.source_span else 0,
        ):
            if orig_overload.source_span and orig_overload.query_literal:
                # This is a concrete overload
                last_overload = orig_overload

        if last_overload and last_overload.source_span:
            # Insert right after the last overload
            insertion_offset = last_overload.source_span.end_offset
        else:
            # No concrete overloads found - can't determine insertion point safely
            # Fall back to full regeneration by returning None
            return None, ""

        # Generate the overload methods text
        overload_texts = []
        for overload in sorted(overloads, key=lambda o: o.query_literal):
            overload_node = self.ast_serializer._serialize_overload(overload)
            # Fix missing location information for ast.unparse()
            ast.fix_missing_locations(overload_node)
            overload_code = ast.unparse(overload_node)
            overload_lines = overload_code.split("\n")

            # Add proper indentation (4 spaces for class methods)
            indented_lines = ["    " + line if line.strip() else "" for line in overload_lines]
            overload_texts.append("\n".join(indented_lines))

        # Combine all overloads with blank lines between them
        new_text = "\n\n" + "\n\n".join(overload_texts)

        return insertion_offset, new_text

    def _add_blank_lines(self, lines: list[str]) -> list[str]:
        """Add blank lines between sections for PEP 8 compliance.

        Args:
            lines: List of code lines

        Returns:
            List of code lines with appropriate blank lines
        """
        result: list[str] = []
        prev_line_type = None

        for line in lines:
            line_type = self._classify_line(line)

            if prev_line_type is not None and line_type != prev_line_type:
                if prev_line_type == "import" and line_type == "import":
                    # Different import groups
                    pass
                elif prev_line_type == "import" and line_type != "import":
                    # Between imports and code
                    result.append("")
                elif prev_line_type == "function" and line_type == "function":
                    # Between functions
                    result.append("")
                    result.append("")
                elif prev_line_type == "function" and line_type == "alias":
                    # Between function and alias
                    result.append("")

            result.append(line)
            prev_line_type = line_type

        return result

    def _classify_line(self, line: str) -> Literal["import", "function", "alias", "other"]:
        """Classify a line of code.

        Args:
            line: Line of code

        Returns:
            Classification: "import", "function", "alias", or "other"
        """
        stripped = line.strip()
        if stripped.startswith("import ") or stripped.startswith("from "):
            return "import"
        elif stripped.startswith("def "):
            return "function"
        elif " = " in stripped and "." in stripped:
            # Likely an alias like "sql = Query.sql"
            return "alias"
        return "other"
