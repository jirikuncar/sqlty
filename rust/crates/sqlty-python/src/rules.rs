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
}

/// Rule severity configuration: the defaults plus the user's overrides.
#[derive(Clone, Debug)]
pub struct RuleConfig {
    enabled: RuleSet,
    severity: [Severity; Rule::COUNT],
}

impl Default for RuleConfig {
    fn default() -> Self {
        RuleConfig {
            enabled: Rule::ALL
                .into_iter()
                .filter(|rule| rule.default_severity() != Severity::Ignore)
                .collect(),
            severity: Rule::ALL.map(Rule::default_severity),
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
