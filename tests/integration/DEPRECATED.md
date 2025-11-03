"""Migration notice for integration tests.

DEPRECATED: These integration tests import from internal modules (sqlty._generator).

According to the acceptance criteria for issue "Refactor tests to use inline-snapshot 
and mypy check", tests should NOT import any file starting with `_`.

New end-to-end tests have been created in tests/e2e/ that follow the correct approach:
- No internal module imports
- Subprocess-based execution (real CLI usage)
- Inline snapshot testing
- Mypy validation

These old tests are kept for now to maintain compatibility, but new tests should
be added to tests/e2e/ instead.

See tests/e2e/README.md for the new testing approach.
"""
