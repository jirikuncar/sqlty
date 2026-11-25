//! Lint rules and their severities, as `[tool.sqlty.rules]` names them.
//!
//! The rules themselves come from the rule pages, through `sqlty-rules`. Python hands the
//! engine rule names and severities as strings; they are parsed once, at the boundary,
//! into a [`RuleConfig`], so an unknown rule or severity is an error there and the checker
//! and fixer only deal with enums.

use std::collections::HashMap;

pub use sqlty_rules::{Group, Rule, RuleSet, Severity};

/// The rules the SQL analyzer reports.
pub const SQL_RULES: RuleSet = {
    let mut set = RuleSet::EMPTY;
    let mut rest = Rule::SQL;
    while let [rule, tail @ ..] = rest {
        set = set.with(*rule);
        rest = tail;
    }
    set
};

/// The override key of the longest `lock_timeout` that `missing-lock-timeout` accepts, as
/// `[tool.sqlty.rules.missing-lock-timeout] max` is passed along.
pub const LOCK_TIMEOUT_MAX: &str = "missing-lock-timeout.max";

/// A rule name or severity in the overrides that sqlty does not know.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleConfigError {
    /// The overrides name a rule that does not exist.
    #[error("unknown rule: {0}")]
    UnknownRule(String),
    /// The overrides give a rule a severity other than `error`, `warn` or `ignore`.
    #[error("unknown severity for rule {rule}: {severity} (expected error, warn or ignore)")]
    UnknownSeverity {
        /// The rule the severity was given to.
        rule: String,
        /// The severity as written.
        severity: String,
    },
    /// `missing-lock-timeout.max` is not a duration.
    #[error("invalid max for rule missing-lock-timeout: {0} (expected a duration such as '2s')")]
    InvalidMax(String),
}

/// Rule severity configuration: the defaults plus the user's overrides.
#[derive(Clone, Debug)]
pub struct RuleConfig {
    enabled: RuleSet,
    severity: [Severity; Rule::COUNT],
    /// The longest `lock_timeout` a migration may set, such as `2s`.
    pub lock_timeout_max: Option<String>,
}

impl Default for RuleConfig {
    fn default() -> Self {
        RuleConfig {
            enabled: Rule::ALL
                .into_iter()
                .filter(|rule| rule.default_severity() != Severity::Ignore)
                .collect(),
            severity: Rule::ALL.map(Rule::default_severity),
            lock_timeout_max: None,
        }
    }
}

impl RuleConfig {
    /// Parse overrides as Python passes them, rule name to severity name.
    ///
    /// Example:
    ///
    /// ```
    /// use sqlty_python::rules::{Rule, RuleConfig, Severity};
    ///
    /// let overrides = [("select-star".to_string(), "error".to_string())].into();
    /// let config = RuleConfig::from_overrides(Some(overrides)).unwrap();
    /// assert_eq!(config.severity(Rule::SelectStar), Severity::Error);
    /// ```
    pub fn from_overrides(
        overrides: Option<HashMap<String, String>>,
    ) -> Result<Self, RuleConfigError> {
        let mut entries: Vec<_> = overrides.unwrap_or_default().into_iter().collect();
        entries.sort();
        let mut config = RuleConfig::default();
        for (rule_name, severity_name) in entries {
            if rule_name == LOCK_TIMEOUT_MAX {
                if sqlty_sql::migrations::lock_timeout_ms(&severity_name).is_none() {
                    return Err(RuleConfigError::InvalidMax(severity_name));
                }
                config.lock_timeout_max = Some(severity_name);
                continue;
            }
            let rule = Rule::parse(&rule_name)
                .ok_or_else(|| RuleConfigError::UnknownRule(rule_name.clone()))?;
            let severity = Severity::parse(&severity_name).ok_or({
                RuleConfigError::UnknownSeverity {
                    rule: rule_name,
                    severity: severity_name,
                }
            })?;
            config.set(rule, severity);
        }
        Ok(config)
    }

    fn set(&mut self, rule: Rule, severity: Severity) {
        self.severity[rule.index()] = severity;
        if severity == Severity::Ignore {
            self.enabled.remove(rule);
        } else {
            self.enabled.insert(rule);
        }
    }

    /// The configuration reporting what either does, each rule at the stricter severity.
    pub fn strictest(&self, other: &Self) -> Self {
        let rank = |severity| match severity {
            Severity::Error => 2,
            Severity::Warn => 1,
            Severity::Ignore => 0,
        };
        let mut merged = self.clone();
        let ms = |max: &String| sqlty_sql::migrations::lock_timeout_ms(max);
        merged.lock_timeout_max = [&self.lock_timeout_max, &other.lock_timeout_max]
            .into_iter()
            .flatten()
            .min_by(|a, b| {
                ms(a)
                    .partial_cmp(&ms(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();
        for rule in Rule::ALL {
            let (own, theirs) = (self.severity(rule), other.severity(rule));
            merged.set(
                rule,
                if rank(theirs) > rank(own) {
                    theirs
                } else {
                    own
                },
            );
        }
        merged
    }

    /// The effective severity of a rule.
    pub fn severity(&self, rule: Rule) -> Severity {
        self.severity[rule.index()]
    }

    /// Whether a rule reports at all (not `ignore`).
    pub fn is_enabled(&self, rule: Rule) -> bool {
        self.enabled.contains(rule)
    }

    /// The rules that report at all.
    pub fn enabled(&self) -> RuleSet {
        self.enabled
    }

    /// Whether a diagnostic of the SQL analyzer is shown: not where a rule reporting the
    /// same mistake is on.
    pub(crate) fn shows(&self, found: &sqlty_sql::diagnostics::NameDiagnostic) -> bool {
        !found.shadowed_by.is_some_and(|rule| self.is_enabled(rule))
    }

    /// This configuration with every rule but `rules` ignored.
    pub(crate) fn only(&self, rules: &[Rule]) -> RuleConfig {
        let mut config = self.clone();
        for rule in Rule::ALL {
            if !rules.contains(&rule) {
                config.set(rule, Severity::Ignore);
            }
        }
        config
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn the_shortest_lock_timeout_max_wins() {
        let config = |max: Option<&str>| {
            let overrides = max.map(|max| [(LOCK_TIMEOUT_MAX.to_string(), max.to_string())].into());
            RuleConfig::from_overrides(overrides).unwrap()
        };
        for (left, right, expected) in [
            (Some("1min"), Some("2s"), Some("2s")),
            (Some("500"), Some("1s"), Some("500")),
            (None, Some("2s"), Some("2s")),
            (None, None, None),
        ] {
            for merged in [
                config(left).strictest(&config(right)),
                config(right).strictest(&config(left)),
            ] {
                assert_eq!(merged.lock_timeout_max.as_deref(), expected);
            }
        }
        let overrides = [(LOCK_TIMEOUT_MAX.to_string(), "soon".to_string())].into();
        assert_eq!(
            RuleConfig::from_overrides(Some(overrides)).unwrap_err(),
            RuleConfigError::InvalidMax("soon".to_string())
        );
    }

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
}
