"""The name rules the engine reports are registered, and off by default."""

from sqlty._lint import NAME_RULES, RULES, RuleConfig, Severity


def test_name_rules_are_registered_and_off_by_default() -> None:
    assert len(NAME_RULES) == 8
    config = RuleConfig()
    for name in NAME_RULES:
        assert name in RULES
        assert config.severity(name) is Severity.IGNORE
