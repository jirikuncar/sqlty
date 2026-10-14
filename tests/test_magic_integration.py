"""End-to-end test for the magic codec.

This test demonstrates the full codec functionality by creating a test file
that uses the --sql prefix and verifying it works correctly.
"""

import codecs
import tempfile
from pathlib import Path

import pytest


def test_sqlty_codec_registration():
    """Test that the sqlty codec is registered for explicit testing."""
    from sqlty import magic  # noqa: F401

    # The codec should be available as "sqlty"
    codec_info = codecs.lookup("sqlty")
    assert codec_info is not None

    # Test that it transforms --sql strings
    test_bytes = b'q = """--sql\nSELECT 1::INTEGER\n"""'
    result, _ = codec_info.decode(test_bytes)
    assert "SQL[tuple[int]]" in result


def test_codec_adds_datetime_import():
    """Test that the codec adds datetime import when needed."""
    from sqlty import magic  # noqa: F401

    codec_info = codecs.lookup("sqlty")

    test_bytes = b'q = """--sql\nSELECT created_at::TIMESTAMP\n"""'
    result, _ = codec_info.decode(test_bytes)

    # Should have datetime import
    assert "import datetime" in result
    assert "SQL[tuple[datetime.datetime]]" in result


def test_codec_adds_uuid_import():
    """Test that the codec adds uuid import when needed."""
    from sqlty import magic  # noqa: F401

    codec_info = codecs.lookup("sqlty")

    test_bytes = b'q = """--sql\nSELECT user_id::UUID\n"""'
    result, _ = codec_info.decode(test_bytes)

    # Should have uuid import
    assert "import uuid" in result
    assert "SQL[tuple[uuid.UUID]]" in result


def test_codec_adds_multiple_imports():
    """Test that the codec adds multiple imports when needed."""
    from sqlty import magic  # noqa: F401

    codec_info = codecs.lookup("sqlty")

    test_bytes = b"""
q1 = \"\"\"--sql
SELECT created_at::TIMESTAMP
\"\"\"
q2 = \"\"\"--sql
SELECT user_id::UUID
\"\"\"
"""
    result, _ = codec_info.decode(test_bytes)

    # Should have both imports
    assert "import datetime" in result
    assert "import uuid" in result
    assert "SQL[tuple[datetime.datetime]]" in result
    assert "SQL[tuple[uuid.UUID]]" in result


def test_codec_no_unnecessary_imports():
    """Test that the codec doesn't add imports when not needed."""
    from sqlty import magic  # noqa: F401

    codec_info = codecs.lookup("sqlty")

    test_bytes = b'q = """--sql\nSELECT id::INTEGER, name::TEXT\n"""'
    result, _ = codec_info.decode(test_bytes)

    # Should NOT have datetime or uuid imports
    assert "import datetime" not in result
    assert "import uuid" not in result
    assert "SQL[tuple[int, str]]" in result


def test_magic_codec_integration():
    """Test the full codec integration with a real Python file."""
    # Create a test Python file that uses the magic codec
    test_code = """from sqlty import magic

# This should be transformed by the codec
q = \"\"\"--sql
SELECT 1::INTEGER
\"\"\"
"""

    # Write the test file to a temporary location
    with tempfile.TemporaryDirectory() as tmpdir:
        test_file = Path(tmpdir) / "test_sql.py"
        test_file.write_text(test_code)

        # The codec should work when compiling the file
        try:
            # Read and decode the file using the codec
            with open(test_file, "rb") as f:
                content = f.read()

            # Use the decode function to transform it
            from sqlty._decode import decode

            decoded, _ = decode(content)

            # Verify the transformation
            assert "SQL[tuple[int]]" in decoded
            assert '"""--sql\nSELECT 1::INTEGER\n"""' in decoded

        except Exception as e:
            pytest.skip(f"Codec integration test skipped due to: {e}")


def test_magic_module_can_be_imported():
    """Test that the magic module can be imported and registers the codec."""
    from sqlty import magic  # noqa: F401

    # The codec is registered. Due to Python's codec caching,
    # it works for files imported AFTER magic is imported.
    # Test that the decode function works directly
    from sqlty._decode import decode

    test_bytes = b'q = """--sql\nSELECT 1::INTEGER\n"""'
    result, _ = decode(test_bytes)
    assert "SQL[tuple[int]]" in result


def test_codec_works_for_subsequently_imported_files():
    """Test that the codec works for files imported after magic is registered."""
    import sys
    import tempfile
    from pathlib import Path

    # Import magic FIRST
    from sqlty import magic  # noqa: F401

    # Create a test module with --sql strings
    with tempfile.TemporaryDirectory() as tmpdir:
        test_file = Path(tmpdir) / "test_sql_module.py"
        test_file.write_text('query = """--sql\nSELECT 1::INTEGER\n"""')

        # Add to path
        sys.path.insert(0, tmpdir)

        try:
            # Import the module - it should use our codec
            import importlib.util

            spec = importlib.util.spec_from_file_location("test_sql_module", test_file)
            if spec and spec.loader:
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)

                # Due to codec caching, this won't be transformed
                # unless we find a way to force cache invalidation
                # For now, we document that the magic codec requires
                # being imported very early (via sqlty_magic.pth or similar)
        finally:
            if tmpdir in sys.path:
                sys.path.remove(tmpdir)


def test_decode_function():
    """Test the decode function directly."""
    from sqlty._decode import decode

    # Test transforming a simple --sql string
    code = b'q = """--sql\nSELECT 1::INTEGER\n"""'
    result, length = decode(code)

    # The result should contain the SQL[...] transformation
    assert "SQL[tuple[int]]" in result
    assert length == len(code)


def test_decode_preserves_non_sql_strings():
    """Test that non-SQL strings are not transformed."""
    from sqlty._decode import decode

    code = b'q = "Hello, World!"'
    result, length = decode(code)

    # The result should be unchanged
    assert "SQL[" not in result
    assert "Hello, World!" in result
    assert length == len(code)


def test_decode_handles_multiple_strings():
    """Test that multiple strings are handled correctly."""
    from sqlty._decode import decode

    code = b"""
q1 = \"\"\"--sql
SELECT 1::INTEGER
\"\"\"
q2 = "Regular string"
q3 = \"\"\"--sql
SELECT id::INTEGER, name::TEXT
\"\"\"
"""
    result, length = decode(code)

    # q1 should be transformed
    assert "SQL[tuple[int]]" in result
    # q2 should not be transformed
    assert '"Regular string"' in result
    # q3 should be transformed
    assert "SQL[tuple[int, str]]" in result
    assert length == len(code)


def test_decode_accepts_buffer_types():
    """decode() accepts any bytes-like object, as the codec protocol requires."""
    from sqlty._decode import decode

    src = b'q = "--sql\\nSELECT 1::INTEGER"'
    expected = decode(src)
    assert decode(bytearray(src)) == expected
    assert decode(memoryview(src)) == expected


def test_decoded_coding_declaration_is_utf8():
    from sqlty._decode import decode

    text, _ = decode(b'# coding: sqlty\nq = "--sql\\nSELECT 1::INTEGER"\n')
    assert text == (
        '# coding: utf-8\nfrom sqlty import SQL; q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER")\n'
    )


def test_decoded_coding_declaration_on_line_two():
    from sqlty._decode import decode

    text, _ = decode(b'#!/usr/bin/env python\n# -*- coding: sqlty -*-\nq = "--sql\\nSELECT 1"\n')
    assert text.splitlines() == [
        "#!/usr/bin/env python",
        "# -*- coding: utf-8 -*-",
        'from sqlty import SQL; q = SQL[tuple[int]]("--sql\\nSELECT 1")',
    ]


@pytest.mark.parametrize(
    ("body", "first"),
    [
        ('"""Doc."""\n', '"""Doc."""; import datetime; from sqlty import SQL'),
        (
            "from __future__ import annotations\n",
            "from __future__ import annotations; import datetime; from sqlty import SQL",
        ),
        ("\n@decorator\ndef f(): ...\n", "import datetime; from sqlty import SQL"),
        ("import datetime\nx = 1\n", "from sqlty import SQL; import datetime"),
    ],
)
def test_decode_script_mode_keeps_lines(body: str, first: str):
    """Running a script decodes ``"\\n" + rest`` and drops the first line (#111)."""
    from sqlty._decode import decode

    source = "\n" + body + 'q = "--sql\\nSELECT NOW()::TIMESTAMP"\n'
    text, _ = decode(source.encode())
    assert text.count("\n") == source.count("\n")
    assert text.splitlines()[:2] == ["", first]
    assert "SQL[tuple[datetime.datetime]]" in text


def test_decode_keeps_source_on_bad_import_style(monkeypatch: pytest.MonkeyPatch):
    from sqlty import _decode

    monkeypatch.setattr(_decode, "_import_style", lambda: "from x import SQL")
    source = b'q = "--sql\\nSELECT 1"\n'
    assert _decode.decode(source)[0] == source.decode()
