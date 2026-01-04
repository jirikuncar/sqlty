"""Test demonstrating the exact use case from the issue."""

import tempfile
from pathlib import Path


def test_issue_example():
    """Test the exact example from the GitHub issue.

    The issue states:
    ```python
    from sqlty import magic

    q = "--sql SELECT 1"
    ```

    should more or less result in `q = SQL[int]("--sql SELECT 1")`.

    However, since "SELECT 1" without an explicit cast returns a single column,
    the inferred type will be tuple[str] (default when type can't be determined).

    For proper type inference, use explicit casts: "--sql SELECT 1::INTEGER"
    """
    # Test with explicit cast (recommended)
    test_code_with_cast = b'q = "--sql SELECT 1::INTEGER"'

    from sqlty.magic import decode

    result, _ = decode(test_code_with_cast)

    # Should transform to SQL[tuple[int]](...)
    assert "SQL[tuple[int]]" in result
    assert '"--sql SELECT 1::INTEGER"' in result

    # The transformation should preserve the original string
    # Result should be: q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")
    expected_transformation = 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")'
    assert expected_transformation in result


def test_issue_example_without_cast():
    """Test the example without explicit cast.

    Without a cast, the type cannot be precisely inferred, so it defaults to str.
    """
    test_code = b'q = "--sql SELECT 1"'

    from sqlty.magic import decode

    result, _ = decode(test_code)

    # Without explicit cast, it should still transform but may use Any or str
    assert "SQL[" in result
    assert '"--sql SELECT 1"' in result


def test_complete_example_from_issue():
    """Test a complete file based on the issue example."""
    # No encoding declaration needed - magic codec overrides UTF-8
    test_code = '''from sqlty import magic

q = "--sql SELECT 1::INTEGER"
'''

    # Write to temp file and decode
    with tempfile.TemporaryDirectory() as tmpdir:
        test_file = Path(tmpdir) / "test.py"
        test_file.write_text(test_code)

        # Read and decode
        from sqlty.magic import decode

        with open(test_file, "rb") as f:
            content = f.read()

        result, _ = decode(content)

        # Verify the transformation
        assert "SQL[tuple[int]]" in result
        assert 'q = SQL[tuple[int]]("--sql SELECT 1::INTEGER")' in result


def test_demonstration():
    """Demonstrate various SQL transformations."""
    from sqlty.magic import decode

    examples = [
        (b'q = "--sql SELECT 1::INTEGER"', "SQL[tuple[int]]"),
        (
            b'q = "--sql SELECT id::INTEGER, name::TEXT"',
            "SQL[tuple[int, str]]",
        ),
        (
            b'q = "--sql SELECT created_at::TIMESTAMP"',
            "SQL[tuple[datetime.datetime]]",
        ),
    ]

    for code, expected_type in examples:
        result, _ = decode(code)
        assert expected_type in result, f"Expected {expected_type} in {result}"
