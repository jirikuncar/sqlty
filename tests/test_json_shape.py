"""``json_problems``: what keeps an annotation from holding the JSON a query builds."""

from __future__ import annotations

import ast

import pytest

from sqlty._json_shape import json_problems

INTEGER = '{"shape":"integer","nullable":false}'
NULLABLE = '{"shape":"integer","nullable":true}'
OBJECT = f'{{"object":[{{"name":"a","value":{INTEGER},"optional":false}}]}}'
TYPING = {name: name for name in ("Optional", "Union", "Literal")}


@pytest.mark.parametrize(
    ("shape", "annotation", "problems"),
    [
        ('"integer"', "1", []),
        ('"integer"', "'int'", []),
        ('"integer"', "'int |'", []),
        ('"string"', "Optional[int]", ["is int but the query builds a string"]),
        ('"string"', "Union[int, None]", ["is int but the query builds a string"]),
        (f'{{"array":{NULLABLE}}}', "list[Literal[1, None]]", []),
        (f'{{"array":{NULLABLE}}}', "list[None]", ["[] is None but the query builds an integer"]),
        (OBJECT, "dict", []),
        (
            OBJECT,
            "dict[int, str]",
            [
                "has keys of int but JSON object keys are strings",
                "a is str but the query builds an integer",
            ],
        ),
        ('"integer"', "int | " * 40 + "str", []),
    ],
)
def test_problems(shape: str, annotation: str, problems: list[str]) -> None:
    assert json_problems(shape, ast.parse(annotation, mode="eval").body, {}, TYPING) == problems


def test_nesting_deeper_than_the_limit_is_not_checked() -> None:
    shape = '"integer"'
    for _ in range(40):
        shape = f'{{"array":{{"shape":{shape},"nullable":false}}}}'
    annotation = "list[" * 40 + "str" + "]" * 40
    assert json_problems(shape, ast.parse(annotation, mode="eval").body, {}, {}) == []
