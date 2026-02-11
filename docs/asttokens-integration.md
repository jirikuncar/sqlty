# AST Node References Integration

> **Status:** This document describes the asttokens integration for the stub generation pipeline, which is deferred.
> See [PLAN-FUTURE.md](../PLAN-FUTURE.md) for the stub generation roadmap.

This document describes the integration of `asttokens` for tracking exact source ranges in SQLTy.

## Overview

SQLTy now uses the `asttokens` library to track precise source locations for:
- SQL query calls in source files
- Overload signatures in stub files

This enables future features like:
- Surgical editing of stub files to preserve comments
- Better error messages with exact source locations
- Incremental updates that don't modify unrelated code

## Implementation Details

### SourceSpan Dataclass

A new `SourceSpan` dataclass in `_stub_ir.py` stores:
- `start_offset`: Starting byte offset in source file
- `end_offset`: Ending byte offset in source file
- `start_line`: Starting line number (1-indexed)
- `start_col`: Starting column number (0-indexed)
- `end_line`: Ending line number (1-indexed)
- `end_col`: Ending column number (0-indexed)

### OverloadSignature Updates

The `OverloadSignature` class now includes:
- `source_span`: Optional `SourceSpan` field storing precise location

### Parser Integration

#### PyiToIRParser (_stub_parser.py)

- Creates an `ASTTokens` object when parsing stub files
- Extracts source spans for each overload using `get_text_range()` and `offset_to_line()`
- Stores spans in `OverloadSignature.source_span`

#### LiteralCallCollector (_collector.py)

- Accepts optional `ASTTokens` object in constructor
- Extracts source spans for each SQL call
- Adds span information to `CallInfo` dict

#### TypeStubGenerator (_generator.py)

- Creates `ASTTokens` when analyzing source files
- Passes it to `LiteralCallCollector`

### Serializer Updates

The `IRToTextSerializer` now supports a `preserve_structure` flag that will:
- Use surgical edits based on source spans
- Preserve comments and formatting
- Only modify lines that need to change

**Note**: Full surgical editing is not yet implemented. The infrastructure is in place for future enhancement.

## Current Behavior

### Incremental Mode
Currently regenerates the entire stub file but:
- Preserves all existing overloads
- Adds new overloads
- Does NOT modify or remove existing overloads

### Prune Mode
Currently regenerates the entire stub file but:
- Keeps overloads that still exist in source
- Removes overloads that are no longer in source
- Does NOT add new overloads

## Future Enhancements

With source spans tracked, future versions can:

1. **Preserve Comments**: Use source spans to locate and preserve comments in stub files
2. **Minimal Edits**: Only edit the specific lines that need changes
3. **Better Diffs**: Generate cleaner git diffs by preserving formatting
4. **Error Messages**: Show exact source locations in error messages
5. **IDE Integration**: Support jump-to-definition from stub to source

## Testing

Comprehensive tests in `tests/ir/test_stub_ir.py` verify:
- Source span tracking in pyi files
- Source span propagation from CallInfo
- Source span stability across edits
- Incremental mode preserves existing overloads
- Prune mode only removes stale overloads

## Dependencies

- `asttokens>=3.0.0`: Added to `pyproject.toml` dependencies
