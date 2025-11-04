"""Notice about test structure.

These tests import from internal modules (starting with `_`) and test internal
implementation details. While these tests provide value for unit testing, they
violate the acceptance criteria that states tests should not import internal files.

For new functionality and end-to-end testing, please add tests to tests/e2e/
instead, which follow the correct approach:
- No internal module imports
- Subprocess-based CLI execution
- Inline snapshot testing
- Mypy validation

See tests/e2e/README.md for more information.
"""
