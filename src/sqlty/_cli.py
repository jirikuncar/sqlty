"""``sqlty``: the commands of the Rust CLI, run in this interpreter."""

from __future__ import annotations

import os
import sys
from typing import NoReturn

EXIT_ERROR = 2
"""Bad usage or configuration, a missing path, or an internal error."""


def main(argv: list[str] | None = None) -> None:
    """CLI entry point."""
    from sqlty._core import cli

    hard_exit = argv is None
    args = sys.argv[1:] if argv is None else argv
    try:
        code = cli(["sqlty", *args])
        sys.stdout.flush()
    except BrokenPipeError:
        _discard_stdout()
        code = 1
    except KeyboardInterrupt, SystemExit:
        raise
    except BaseException as exc:  # a Rust panic is a BaseException
        import traceback

        traceback.print_exc()
        print(
            "sqlty: internal error; please report it with the traceback above at "
            + issue_url(f"{type(exc).__name__}: {exc}"),
            file=sys.stderr,
        )
        code = EXIT_ERROR
    if hard_exit:
        _hard_exit(code)
    if code:
        sys.exit(code)


def issue_url(cause: str) -> str:
    """A new-issue link prefilling the bug report form, as ``sqlty_python::report`` does."""
    import platform
    from urllib.parse import quote, urlencode

    from sqlty import __version__

    title = cause.splitlines()[0]
    fields = {
        "title": f"Crash: {title[:80]}",
        "output": cause[:2000],
        "version": __version__,
        "environment": f"{platform.system()} {platform.machine()}, Python {platform.python_version()}",
    }
    return "https://github.com/jirikuncar/sqlty/issues/new?template=bug_report.yml&" + urlencode(
        fields, quote_via=quote, safe=""
    )


def _discard_stdout() -> None:
    """Send what is left of stdout to the null device once its reader is gone, as the
    ``signal`` docs advise, so the interpreter does not fail flushing it at exit."""
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, sys.stdout.fileno())


def _hard_exit(code: int) -> NoReturn:
    """End the process without freeing what the run built, as mypy's ``hard_exit`` does:
    freeing the index of a large project takes about as long as checking it."""
    from sqlty._core import write_profile

    sys.stdout.flush()
    sys.stderr.flush()
    write_profile()
    os._exit(code)
