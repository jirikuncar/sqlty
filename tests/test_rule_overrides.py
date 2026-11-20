"""The engine rejects rule overrides it cannot read instead of guessing."""

import pytest

from sqlty import core
from sqlty._lint import RULES

SOURCE = 'q = "--sql\\nSELECT 1::INTEGER"\n'


def test_unknown_severity_is_a_value_error() -> None:
    with pytest.raises(ValueError, match="unknown severity for rule stale-rewrite: loud"):
        core.check_source(SOURCE, rules={"stale-rewrite": "loud"})


def test_unknown_rule_is_a_value_error() -> None:
    with pytest.raises(ValueError, match="unknown rule: no-such-rule"):
        core.check_source(SOURCE, rules={"no-such-rule": "error"})


def test_fix_source_rejects_unknown_severity() -> None:
    with pytest.raises(ValueError, match="unknown severity"):
        core.fix_source(SOURCE, rules={"stale-rewrite": "loud"})


def test_valid_overrides_still_apply() -> None:
    assert core.check_source(SOURCE, rules={"stale-rewrite": "ignore"}) == []
    [diagnostic] = core.check_source(SOURCE, rules={"stale-rewrite": "error"})
    assert diagnostic["severity"] == "error"


def test_an_override_leaves_the_other_rules_at_their_defaults() -> None:
    [diagnostic] = core.check_source(SOURCE, rules={"stale-type": "error"})
    assert diagnostic["rule"] == "stale-rewrite"
    assert diagnostic["severity"] == RULES["stale-rewrite"].default_severity.value
