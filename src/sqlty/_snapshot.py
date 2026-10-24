"""Snapshots for ``sqlty breaking``: what ``--save`` writes and ``--against`` reads.

A snapshot holds what checking a revision needs, so a release is a baseline without its
git history: its Python files, the settings each is checked with, and the schemas they
name. A file the snapshot lacks takes the schema of its nearest directory.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING, TypedDict, cast

from sqlty._breaking import Source

if TYPE_CHECKING:
    from sqlty._breaking import Revision
    from sqlty._cli import Settings
    from sqlty.core import Schema

FORMAT = 1


class SchemaEntry(TypedDict):
    sql: str
    dialect: str
    types: dict[str, str] | None


class SettingsEntry(TypedDict):
    dialect: str
    schema: int | None
    import_style: str | None
    untyped: bool
    rules: dict[str, str] | None
    type_modules: list[str]
    functions: list[tuple[str, str, str | None]]


class FileEntry(TypedDict):
    source: str
    module: str
    names: list[str]
    settings: int


class SnapshotData(TypedDict):
    sqlty: int
    schemas: list[SchemaEntry]
    settings: list[SettingsEntry]
    directories: dict[str, int | None]
    files: dict[str, FileEntry]


def save(revision: Revision, path: Path) -> None:
    """Write the snapshot of ``revision`` to ``path``."""
    schemas: list[SchemaEntry] = []
    settings: list[SettingsEntry] = []

    def schema(s: Settings) -> int | None:
        if s.schema_sql is None:
            return None
        return _index(schemas, SchemaEntry(sql=s.schema_sql, dialect=s.dialect, types=s.types))

    def entry(s: Settings) -> int:
        return _index(
            settings,
            SettingsEntry(
                dialect=s.dialect,
                schema=schema(s),
                import_style=s.import_style,
                untyped=s.untyped,
                rules=s.rules,
                type_modules=list(s.type_modules),
                functions=list(s.functions),
            ),
        )

    sources = revision.sources()
    files = {
        relative.as_posix(): FileEntry(
            source=source.text,
            module=source.module,
            names=list(source.names),
            settings=entry(revision.settings(relative)),
        )
        for relative, source in sources.items()
    }
    directories = sorted({Path(".")} | {d for relative in sources for d in relative.parents})
    data = SnapshotData(
        sqlty=FORMAT,
        schemas=schemas,
        settings=settings,
        directories={
            d.as_posix(): schema(revision.settings(d / "__init__.py")) for d in directories
        },
        files=files,
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")


@dataclass
class Snapshot:
    """A revision read back from its snapshot, limited to ``paths``."""

    label: str
    data: SnapshotData
    paths: list[Path]
    loaded: dict[int, Schema] = field(default_factory=dict)

    @classmethod
    def load(cls, path: Path, label: str, paths: list[Path]) -> Snapshot:
        from sqlty._config import ConfigError

        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise ConfigError(f"{path} is not a sqlty snapshot: {exc}") from None
        if not isinstance(data, dict) or data.get("sqlty") != FORMAT:
            raise ConfigError(f"{path} is not a sqlty snapshot of format {FORMAT}")
        return cls(label, cast("SnapshotData", data), paths)

    def sources(self) -> dict[Path, Source]:
        return {
            relative: Source(file["source"], file["module"], tuple(file["names"]))
            for name, file in self.data["files"].items()
            if any(p == (relative := Path(name)) or p in relative.parents for p in self.paths)
        }

    def roots(self) -> list[Path]:
        """A snapshot keeps the checked files, not the directories they were read from."""
        return []

    def settings(self, relative: Path) -> Settings:
        from sqlty._cli import Settings

        entry = self.data["settings"][self.data["files"][relative.as_posix()]["settings"]]
        return Settings(
            dialect=entry["dialect"],
            schema=self._schema(entry["schema"]),
            import_style=entry["import_style"],
            untyped=entry["untyped"],
            rules=entry["rules"],
            type_modules=tuple(entry["type_modules"]),
            functions=tuple((f[0], f[1], f[2]) for f in entry["functions"]),
        )

    def schema(self, relative: Path, own: Settings) -> Schema | None:
        if (file := self.data["files"].get(relative.as_posix())) is not None:
            return self._schema(self.data["settings"][file["settings"]]["schema"])
        directories = self.data["directories"]
        nearest = next((d.as_posix() for d in relative.parents if d.as_posix() in directories), ".")
        return self._schema(directories.get(nearest))

    def _schema(self, index: int | None) -> Schema | None:
        from sqlty.core import load_schema

        if index is None:
            return None
        if index not in self.loaded:
            entry = self.data["schemas"][index]
            self.loaded[index] = load_schema(
                entry["sql"], dialect=entry["dialect"], types=entry["types"]
            )
        return self.loaded[index]


def _index[T](items: list[T], item: T) -> int:
    if item not in items:
        items.append(item)
    return items.index(item)
