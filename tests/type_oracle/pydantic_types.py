"""Pydantic's types, read without importing pydantic.

Each type has two readings. A type checker sees pydantic's ``TYPE_CHECKING`` declarations
(``EmailStr`` is ``str``, ``PastDatetime`` is ``datetime``, ``AnyUrl`` a class of its own);
a ``TypedDict`` or plain dataclass row holds what the driver returns, so it is checked
against those. A Pydantic model or dataclass validates its input instead: ``AnyUrl``
takes a ``str``, ``ByteSize`` an ``int`` or a ``str``. Its lax conversions from there are
`subtype._LAX_FROM`'s, as for any other field.
"""

from __future__ import annotations

import datetime
import decimal
import functools
import ipaddress
import typing
import uuid


def _stand_in(name: str, *bases: type) -> type:
    return type(name, bases, {"__module__": "pydantic"})


_IP_ADDRESS = ipaddress.IPv4Address | ipaddress.IPv6Address
_IP_NETWORK = ipaddress.IPv4Network | ipaddress.IPv6Network
_IP_INTERFACE = ipaddress.IPv4Interface | ipaddress.IPv6Interface
_JSON = list[typing.Any] | dict[str, typing.Any] | str | bool | int | float | None
_URLS = (
    "AnyUrl AnyHttpUrl HttpUrl AnyWebsocketUrl WebsocketUrl FileUrl FtpUrl AmqpDsn ClickHouseDsn "
    "CockroachDsn KafkaDsn MariaDBDsn MongoDsn MySQLDsn NatsDsn PostgresDsn RedisDsn SnowflakeDsn"
)

# name -> (type for a type checker, input of a model)
TYPES: dict[str, tuple[typing.Any, typing.Any]] = {
    **dict.fromkeys(("PositiveInt", "NegativeInt", "NonNegativeInt", "NonPositiveInt"), (int, int)),
    **dict.fromkeys(
        ("PositiveFloat", "NegativeFloat", "NonNegativeFloat", "NonPositiveFloat", "FiniteFloat"),
        (float, float),
    ),
    **dict.fromkeys(
        ("AwareDatetime", "NaiveDatetime", "PastDatetime", "FutureDatetime"),
        (datetime.datetime, datetime.datetime),
    ),
    "PastDate": (datetime.date, datetime.date),
    "FutureDate": (datetime.date, datetime.date),
    **{f"UUID{version}": (uuid.UUID, uuid.UUID) for version in (1, 3, 4, 5, 6, 7, 8)},
    "Base64Str": (str, str | bytes),
    "Base64UrlStr": (str, str | bytes),
    "Base64Bytes": (bytes, str | bytes),
    "Base64UrlBytes": (bytes, str | bytes),
    "ByteSize": (_stand_in("ByteSize", int), int | str),
    "PaymentCardNumber": (_stand_in("PaymentCardNumber", str), str),
    "SecretStr": (_stand_in("SecretStr"), str | bytes),
    "SecretBytes": (_stand_in("SecretBytes"), str | bytes),
    "JsonValue": (_JSON, _JSON),
}

NETWORKS: dict[str, tuple[typing.Any, typing.Any]] = {
    **{name: (_stand_in(name), str) for name in _URLS.split()},
    "EmailStr": (str, str),
    "NameEmail": (_stand_in("NameEmail"), str),
    "IPvAnyAddress": (_IP_ADDRESS, _IP_ADDRESS | str),
    "IPvAnyNetwork": (_IP_NETWORK, _IP_NETWORK | _IP_ADDRESS | str),
    "IPvAnyInterface": (_IP_INTERFACE, _IP_INTERFACE | _IP_NETWORK | _IP_ADDRESS | str),
}

# ``conint(...)`` and the like, by the type of their result; inputs as for that type.
CONSTRAINED: dict[str, type] = {
    "conint": int,
    "confloat": float,
    "condecimal": decimal.Decimal,
    "constr": str,
    "conbytes": bytes,
    "condate": datetime.date,
    "conlist": list,
}

# A model's input of ``Json[T]`` is the JSON text, whatever ``T``.
GENERIC: dict[str, typing.Any] = {"Json": str | bytes}

# Public names of `pydantic.types` and `pydantic.networks` left unknown on purpose:
# metadata, generic wrappers, paths checked against the file system, and functions.
UNKNOWN_TYPES = frozenset(
    {
        "AllowInfNan",
        "Base64Encoder",
        "DirectoryPath",
        "Discriminator",
        "EncodedBytes",
        "EncodedStr",
        "EncoderProtocol",
        "FailFast",
        "FilePath",
        "GetPydanticSchema",
        "ImportString",
        "NewPath",
        "OnErrorOmit",
        "Secret",
        "SocketPath",
        "Strict",
        "StringConstraints",
        "Tag",
        "confrozenset",
        "conset",
    }
)
UNKNOWN_NETWORKS = frozenset({"UrlConstraints", "validate_email"})


@functools.cache
def pydantic_names(model: bool) -> dict[str, typing.Any]:
    """The types `_ast_type` resolves Pydantic's qualified names to; a model's inputs if ``model``.

    ``conint(...)`` is keyed ``pydantic.conint()``; ``Json[T]`` is ``pydantic.Json[]``, for a
    model only.

    Example:
        >>> checker, model = pydantic_names(model=False), pydantic_names(model=True)
        >>> checker["pydantic.EmailStr"], checker["pydantic.networks.AnyUrl"], model["pydantic.AnyUrl"]
        (<class 'str'>, <class 'pydantic.AnyUrl'>, <class 'str'>)
    """
    found: dict[str, typing.Any] = {}
    for module, table in (("types", TYPES), ("networks", NETWORKS)):
        for name, readings in table.items():
            found[f"pydantic.{name}"] = found[f"pydantic.{module}.{name}"] = readings[model]
    for name, tp in CONSTRAINED.items():
        found[f"pydantic.{name}()"] = found[f"pydantic.types.{name}()"] = tp
    if model:
        for name, tp in GENERIC.items():
            for key in (f"pydantic.{name}", f"pydantic.types.{name}"):
                found[key] = found[f"{key}[]"] = tp
    return found
