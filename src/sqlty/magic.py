"""Magic codec for automatic --sql string transformation via Python's codec system.

This module registers a custom Python codec that enables automatic transformation
of strings prefixed with '--sql' into SQL[T](...) calls with type inference.

IMPORTANT: Importing this module automatically registers the codec (unless
SQLTY_NO_MAGIC environment variable is set). If you only need the decode function
without codec registration, use 'sqlty._decode' instead.

Automatic Registration (Default):
    import sqlty.magic  # Registers codec immediately

    # Now use # coding: sqlty in files
    # coding: sqlty
    q = "--sql SELECT 1::INTEGER"
    # Python automatically transforms to: SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")

Disable Auto-Registration:
    import os
    os.environ['SQLTY_NO_MAGIC'] = '1'
    import sqlty.magic  # Won't register codec

    # Or import decode directly without registration:
    from sqlty._decode import decode

Direct Decode Usage:
    from sqlty._decode import decode  # No side effects

    code = b'q = "--sql SELECT 1::INTEGER"'
    transformed, _ = decode(code)

CLI Tool:
    python -m sqlty.magic <filename>
    # Transforms a file and outputs the result

For Global Setup:
    When you install sqlty, a 'sqlty_magic.pth' file is automatically added to your
    site-packages directory. This file imports sqlty.magic on Python startup, enabling
    the codec globally.

    To disable this behavior, set SQLTY_NO_MAGIC=1 environment variable:
        export SQLTY_NO_MAGIC=1

    Then use '# coding: sqlty' in any file for automatic transformation.

See docs/magic-codec.md for complete documentation.
"""

from __future__ import annotations

import codecs
import encodings
import io
import os
import sys
from collections.abc import Buffer
from encodings.utf_8 import StreamReader as _Utf8StreamReader
from typing import TYPE_CHECKING, Any

# Import decode from _decode module to avoid circular imports
from sqlty._decode import decode

# Re-export decode for backward compatibility
__all__ = ["decode", "register"]

if TYPE_CHECKING:
    from codecs import CodecInfo

# Get the standard UTF-8 codec
utf_8_codec = encodings.search_function("utf8")
assert utf_8_codec is not None, "UTF-8 codec not found"


class IncrementalDecoder(codecs.BufferedIncrementalDecoder):
    """Incremental decoder for the sqlty-magic codec."""

    def _buffer_decode(
        self, input: Buffer, errors: str, final: bool
    ) -> tuple[str, int]:  # pragma: no cover
        if final:
            return decode(input, errors)
        else:
            return "", 0


class StreamReader(_Utf8StreamReader):
    """Stream reader for the sqlty-magic codec.

    Decode is deferred to support better error messages.
    """

    _stream: Any = None
    _decoded: bool = False

    @property
    def stream(self) -> Any:
        """Get the decoded stream."""
        if not self._decoded:
            text, _ = decode(self._stream.read())
            self._stream = io.BytesIO(text.encode("UTF-8"))
            self._decoded = True
        return self._stream

    @stream.setter
    def stream(self, stream: Any) -> None:
        """Set the stream to decode."""
        self._stream = stream
        self._decoded = False


# Create the custom codec info that will be used for UTF-8 and sqlty
_custom_codec_info = codecs.CodecInfo(
    name="utf-8",
    encode=utf_8_codec.encode,
    decode=decode,
    incrementalencoder=utf_8_codec.incrementalencoder,
    incrementaldecoder=IncrementalDecoder,
    streamreader=StreamReader,
    streamwriter=utf_8_codec.streamwriter,
)

# Also create a separate codec info for "sqlty" for testing purposes
_sqlty_codec_info = codecs.CodecInfo(
    name="sqlty",
    encode=utf_8_codec.encode,
    decode=decode,
    incrementalencoder=utf_8_codec.incrementalencoder,
    incrementaldecoder=IncrementalDecoder,
    streamreader=StreamReader,
    streamwriter=utf_8_codec.streamwriter,
)


def _codec_search(encoding: str) -> CodecInfo | None:
    """Search function for codec registration.

    This function provides the custom codec for:
    - UTF-8 and its aliases (for automatic transformation)
    - "sqlty" (for explicit testing)

    Args:
        encoding: The encoding name being looked up

    Returns:
        Custom CodecInfo for supported encodings, None otherwise
    """
    # Normalize the encoding name (lowercase, remove hyphens and underscores)
    normalized = encoding.lower().replace("-", "").replace("_", "")

    # Handle UTF-8 and its common aliases
    if normalized in ("utf8", "utf", "u8"):
        return _custom_codec_info

    # Handle "sqlty" for explicit testing
    if normalized == "sqlty":
        return _sqlty_codec_info

    return None


def register() -> None:
    """Register the custom codec with Python's codec registry.

    This attempts to override UTF-8 codec for automatic --sql string transformation.

    Note: Due to Python's codec caching (built-in codecs are registered before user
    code runs), this only works when called very early during Python startup.

    When you install sqlty, a 'sqlty_magic.pth' file is automatically added to your
    site-packages directory, which imports this module on Python startup. This enables
    the codec globally for all Python sessions.

    To disable automatic registration, set SQLTY_NO_MAGIC=1 environment variable
    before starting Python.

    Alternatively, use the decode() function from sqlty._decode directly to transform
    source code without any codec registration.
    """
    import _codecs

    # Register our search function
    codecs.register(_codec_search)

    # Attempt to clear the codec cache
    # This may not work if UTF-8 was already cached before this runs
    try:
        _codecs.unregister(_codec_search)
        codecs.register(_codec_search)
    except Exception:
        # If unregister fails, just continue with registration
        pass


# Automatically register when the module is imported
# Skip registration if SQLTY_NO_MAGIC environment variable is set
if not os.environ.get("SQLTY_NO_MAGIC"):
    register()


def main(argv: list[str]) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Prints transformed source.")
    parser.add_argument("filename")
    args = parser.parse_args(argv)

    with open(args.filename, "rb") as f:
        text, _ = decode(f.read())

    # Write to stdout buffer if available, otherwise stdout
    stdout_buffer = getattr(sys.stdout, "buffer", None)
    if stdout_buffer is not None:
        stdout_buffer.write(text.encode("UTF-8"))
    else:
        sys.stdout.write(text)

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
