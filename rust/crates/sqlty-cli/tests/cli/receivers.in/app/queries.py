from app.store import Cached
from lib import Db
from other import Db as Other
from vendored import Website


def stubbed(db: Db) -> None:
    db.one("SELECT nope FROM users", int)


def inherited(store: Cached) -> None:
    store.one("SELECT name FROM users", int)


def in_a_library(website: Website) -> None:
    website.db.one("SELECT id, name FROM users", int)


def not_a_stub_path(other: Other) -> None:
    other.one("SELECT nope FROM users", int)


def narrowed(thing: object) -> None:
    if isinstance(thing, Db):
        thing.one("SELECT missing FROM users", int)
