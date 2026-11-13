"""The name rules the engine reports are registered, on by default."""

from sqlty._lint import NAME_RULES, RULES, RuleConfig, Severity

# SQL the database rejects is an error; SQL sqlty can't type is a warning. unproven-join
# is off, as a schema without keys would report every join, and entity-mismatch is opt-in.
DEFAULTS = {
    "unknown-table": Severity.ERROR,
    "unknown-column": Severity.ERROR,
    "ambiguous-column": Severity.ERROR,
    "duplicate-alias": Severity.ERROR,
    "insert-missing-column": Severity.ERROR,
    "insert-value-count": Severity.ERROR,
    "ungrouped-column": Severity.ERROR,
    "unknown-function": Severity.WARN,
    "unknown-type": Severity.WARN,
    "unsupported-syntax": Severity.WARN,
    "fanout-aggregate": Severity.WARN,
    "chasm-join": Severity.WARN,
    "unproven-join": Severity.IGNORE,
    "entity-mismatch": Severity.IGNORE,
}


def test_name_rules_are_registered_and_on_by_default() -> None:
    config = RuleConfig()
    assert {name: config.severity(name) for name in NAME_RULES} == DEFAULTS
    assert all(name in RULES for name in NAME_RULES)
