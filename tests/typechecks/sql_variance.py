"""SQL[R, P] is covariant in R and contravariant in P: a narrower row fits a wider one.

mypy: ok
pyright: ok
ty: ok
"""

from typing import Literal, TypedDict

from sqlty import SQL, Bound


class Card(TypedDict):
    kind: Literal["card"]
    card_last4: str


class Iban(TypedDict):
    kind: Literal["iban"]
    iban: str


def payments(query: SQL[Card | Iban]) -> None: ...


def bound(query: Bound[Card | Iban]) -> None: ...


def by_id(query: SQL[Card, tuple[int]]) -> None: ...


cards = SQL[Card]("--sql\nSELECT kind, card_last4 FROM payments")
any_id = SQL[Card, tuple[int | str]]("--sql\nSELECT kind, card_last4 FROM payments WHERE id = %s")

payments(cards)
bound(cards.bind(()))
by_id(any_id)
