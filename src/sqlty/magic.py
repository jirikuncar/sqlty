"""The ``sqlty`` source codec: ``--sql`` strings become ``SQL[T](...)`` at import.

Modules opt in with a coding declaration on their first or second line::

    # coding: sqlty
    q = "--sql\nSELECT 1::INTEGER"  # Python sees SQL[tuple[int]]("--sql\nSELECT 1::INTEGER")

Importing this module registers the codec; the ``sqlty_magic.pth`` file that a
wheel install puts in ``site-packages`` imports it at interpreter startup.
``SQLTY_NO_MAGIC=1`` skips the registration. Other files, UTF-8 ones included,
are never transformed.

``sqlty._decode.decode`` transforms bytes without registering anything, and
``python -m sqlty.magic FILE`` prints a module as the codec decodes it.
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
    return _sqlty_codec_info if encoding == "sqlty" else None


def register() -> None:
    """Register the ``sqlty`` codec; registering twice is harmless."""
    codecs.unregister(_codec_search)
    codecs.register(_codec_search)


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
