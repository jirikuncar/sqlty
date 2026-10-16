"""Which paths of a git work tree its ``.gitignore`` files ignore."""

from __future__ import annotations

from pathlib import Path

from pathspec import GitIgnoreSpec

_Chain = tuple[tuple[Path, GitIgnoreSpec], ...]


class GitIgnore:
    """The ignore rules of the repositories a walk enters, read once per directory.

    A directory's rules are those of ``.git/info/exclude`` and every ``.gitignore``
    from the repository root down to it; a deeper file overrides a shallower one,
    and a later line an earlier one, as in git. Outside a repository nothing is
    ignored.

    Example:
        >>> GitIgnore().ignored(Path("/"), "tmp", is_dir=True)
        False
    """

    def __init__(self) -> None:
        self._chains: dict[Path, _Chain | None] = {}

    def ignored(self, directory: Path, name: str, *, is_dir: bool) -> bool:
        """Whether the entry ``name`` of the resolved ``directory`` is ignored."""
        verdict = False
        for base, spec in self._chain(directory) or ():
            relative = (directory / name).relative_to(base).as_posix()
            if (
                include := spec.check_file(relative + "/" if is_dir else relative).include
            ) is not None:
                verdict = include
        return verdict

    def _chain(self, directory: Path) -> _Chain | None:
        if directory not in self._chains:
            if (directory / ".git").exists():
                chain: _Chain | None = _read(directory, directory / ".git" / "info" / "exclude")
            elif directory.parent == directory:
                chain = None
            else:
                chain = self._chain(directory.parent)
            if chain is not None:
                chain += _read(directory, directory / ".gitignore")
            self._chains[directory] = chain
        return self._chains[directory]


def _read(base: Path, path: Path) -> _Chain:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError, UnicodeDecodeError:
        return ()
    return ((base, GitIgnoreSpec.from_lines(lines)),)
