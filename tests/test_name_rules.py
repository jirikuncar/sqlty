"""The name rules the engine reports are registered, on by default."""

from tests._core_views import NAME_RULES, RULES

# SQL the database rejects is an error; SQL sqlty can't type is a warning. unproven-join
# is off, as a schema without keys would report every join, and entity-mismatch is opt-in.
DEFAULTS = {
    "unknown-table": "error",
    "unknown-column": "error",
    "ambiguous-column": "error",
    "duplicate-alias": "error",
    "insert-missing-column": "error",
    "insert-value-count": "error",
    "ungrouped-column": "error",
    "unknown-function": "warn",
    "unknown-type": "warn",
    "unsupported-syntax": "warn",
    "fanout-aggregate": "warn",
    "chasm-join": "warn",
    "unproven-join": "ignore",
    "entity-mismatch": "ignore",
    "foreign-key-mismatch": "warn",
    "rejected-query": "error",
}


def test_name_rules_are_registered_and_on_by_default() -> None:
    assert {name: RULES[name].default_severity for name in NAME_RULES} == DEFAULTS
