use crate::common::*;
use sqlty_python::rules::*;

#[test]
fn the_strictest_severity_of_two_configurations_wins_in_either_order() {
    let config = |severity: &str| {
        let overrides = [("select-star".to_string(), severity.to_string())].into();
        RuleConfig::from_overrides(Some(overrides)).unwrap()
    };
    for (left, right, expected) in [
        ("ignore", "warn", Severity::Warn),
        ("warn", "error", Severity::Error),
        ("error", "ignore", Severity::Error),
        ("ignore", "ignore", Severity::Ignore),
    ] {
        let merged = config(left).strictest(&config(right));
        assert_eq!(merged.severity(Rule::SelectStar), expected);
        assert_eq!(
            merged.is_enabled(Rule::SelectStar),
            expected != Severity::Ignore
        );
        assert_eq!(
            config(right)
                .strictest(&config(left))
                .severity(Rule::SelectStar),
            expected
        );
    }
}

#[test]
fn every_rule_name_round_trips() {
    for rule in Rule::ALL {
        assert_eq!(Rule::parse(rule.name()), Some(rule));
    }
}

#[test]
fn every_severity_name_round_trips() {
    for severity in [Severity::Error, Severity::Warn, Severity::Ignore] {
        assert_eq!(Severity::parse(severity.as_str()), Some(severity));
    }
}

#[test]
fn defaults_match_the_documented_table() {
    let config = RuleConfig::default();
    assert_eq!(config.severity(Rule::SelectStarNoSchema), Severity::Error);
    assert_eq!(config.severity(Rule::StaleRewrite), Severity::Warn);
    assert_eq!(config.severity(Rule::StaleType), Severity::Warn);
    assert_eq!(config.severity(Rule::NarrowType), Severity::Error);
    assert_eq!(config.severity(Rule::UnparsedQuery), Severity::Warn);
    assert_eq!(config.severity(Rule::RedundantOptional), Severity::Ignore);
    assert_eq!(config.severity(Rule::DynamicQuery), Severity::Ignore);
    assert_eq!(config.severity(Rule::UnknownTable), Severity::Error);
    assert_eq!(config.severity(Rule::UnknownFunction), Severity::Warn);
}

#[test]
fn override_replaces_the_default() {
    let overrides = HashMap::from([("stale-rewrite".to_string(), "ignore".to_string())]);
    let config = RuleConfig::from_overrides(Some(overrides)).unwrap();
    assert!(!config.is_enabled(Rule::StaleRewrite));
    assert!(config.is_enabled(Rule::StaleType));
}

#[test]
fn unknown_rule_is_an_error() {
    let overrides = HashMap::from([("no-such-rule".to_string(), "error".to_string())]);
    assert_eq!(
        RuleConfig::from_overrides(Some(overrides)).unwrap_err(),
        RuleConfigError::UnknownRule("no-such-rule".to_string())
    );
}

#[test]
fn unknown_severity_is_an_error() {
    let overrides = HashMap::from([("stale-type".to_string(), "loud".to_string())]);
    let error = RuleConfig::from_overrides(Some(overrides)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown severity for rule stale-type: loud (expected error, warn or ignore)"
    );
}
