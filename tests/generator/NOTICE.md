"""Notice about test structure.

These tests import from internal modules (starting with `_`) and test internal 
implementation details. These provide value for unit testing internal logic.

Tests that covered end-to-end workflow functionality have been removed as they
are now covered by the e2e test suite in tests/e2e/.

For new functionality and end-to-end testing, please add tests to tests/e2e/ 
instead, which follow the correct approach:
- No internal module imports
- Subprocess-based CLI execution
- Inline snapshot testing  
- Mypy validation

See tests/e2e/README.md for more information.
"""
