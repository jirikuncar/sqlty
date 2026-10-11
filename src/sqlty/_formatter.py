import logging
import shlex


def formatter(command: str, text: str, filename: str) -> str:
    """Format generated stub content by piping it through an external command.

    The configured format command must use the {filename} placeholder, which will be
    replaced with the provided filename (shell-quoted). The command is executed with the stub content
    provided on stdin, and the formatted content is read from stdout.

    If the formatter fails, the original text is returned unchanged.
    """
    cmd = command.format(filename=shlex.quote(filename))
    try:
        import subprocess as sp

        result = sp.run(
            cmd,
            shell=True,
            input=text.encode("utf-8"),
            capture_output=True,
            timeout=30,
        )
        if result.returncode != 0:
            # Include both stdout and stderr to aid debugging
            logging.getLogger(__name__).warning(
                "format_command failed (exit %s) for '%s'\n%s%s",
                result.returncode,
                cmd,
                result.stdout.decode("utf-8", errors="ignore"),
                result.stderr.decode("utf-8", errors="ignore"),
            )
            return text
        return result.stdout.decode("utf-8", errors="ignore")
    except FileNotFoundError:
        logging.getLogger(__name__).warning("Formatter executable not found when running: %s", cmd)
        return text
    except Exception as e:
        logging.getLogger(__name__).warning("Could not run formatter '%s': %s", cmd, e)
        return text
