"""The name rules the engine reports are registered, on by default."""

from sqlty._lint import NAME_RULES, RULES, RuleConfig, Severity

# SQL the database rejects is an error; SQL sqlty can't type is a warning.
DEFAULTS = {
    "unknown-table": Severity.ERROR,
    "unknown-column": Severity.ERROR,
    "ambiguous-column": Severity.ERROR,
    "insert-missing-column": Severity.ERROR,
    "ungrouped-column": Severity.ERROR,
    "unknown-function": Severity.WARN,
    "unknown-type": Severity.WARN,
    "unsupported-syntax": Severity.WARN,
}


def test_name_rules_are_registered_and_on_by_default() -> None:
    config = RuleConfig()
    assert {name: config.severity(name) for name in NAME_RULES} == DEFAULTS
    assert all(name in RULES for name in NAME_RULES)
