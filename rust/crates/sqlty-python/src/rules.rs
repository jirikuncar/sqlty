//! Lint rules and their severities, as `[tool.sqlty.rules]` names them.
//!
//! Python hands the engine rule names and severities as strings; they are parsed once, at
//! the boundary, into a [`RuleConfig`], so an unknown rule or severity is an error there
//! and the checker and fixer only deal with enums.

use std::collections::HashMap;

use sqlty_sql::diagnostics::NameRule;

/// A lint rule. The rules sqlty reports from Python (row type checks) are listed here too,
/// so every rule name Python may configure parses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    ColumnCountMismatch,
    ColumnTypeMismatch,
    ColumnNameMismatch,
    TypeddictKeyMismatch,
    TypeddictInheritance,
    NullableMismatch,
    JsonShapeMismatch,
    RedundantOptional,
    LiteralMismatch,
    RedundantLiteral,
    SelectStar,
    SelectStarNoSchema,
    StaleRewrite,
    StaleType,
    MysqlSqlMarker,
    InlineSqlComment,
    FstringSkipped,
    TstringSkipped,
    UncheckedQuery,
    UntypedQuery,
    UnparsedQuery,
    UnusedIgnore,
    ParamCountMismatch,
    ParamTypeMismatch,
    /// A rule explaining why a column is untyped.
    Name(NameRule),
}

impl Rule {
    /// Every rule.
    pub const ALL: [Rule; 30] = [
        Rule::ColumnCountMismatch,
        Rule::ColumnTypeMismatch,
        Rule::ColumnNameMismatch,
        Rule::TypeddictKeyMismatch,
        Rule::TypeddictInheritance,
        Rule::NullableMismatch,
        Rule::JsonShapeMismatch,
        Rule::RedundantOptional,
        Rule::LiteralMismatch,
        Rule::RedundantLiteral,
        Rule::SelectStar,
        Rule::SelectStarNoSchema,
        Rule::StaleRewrite,
        Rule::StaleType,
        Rule::MysqlSqlMarker,
        Rule::InlineSqlComment,
        Rule::FstringSkipped,
        Rule::TstringSkipped,
        Rule::UncheckedQuery,
        Rule::UntypedQuery,
        Rule::UnparsedQuery,
        Rule::UnusedIgnore,
        Rule::ParamCountMismatch,
        Rule::ParamTypeMismatch,
        Rule::Name(NameRule::UnknownTable),
        Rule::Name(NameRule::UnknownColumn),
        Rule::Name(NameRule::AmbiguousColumn),
        Rule::Name(NameRule::UnknownFunction),
        Rule::Name(NameRule::UnknownType),
        Rule::Name(NameRule::UnsupportedSyntax),
    ];

    /// The kebab-case rule name.
    pub fn name(self) -> &'static str {
        match self {
            Rule::ColumnCountMismatch => "column-count-mismatch",
            Rule::ColumnTypeMismatch => "column-type-mismatch",
            Rule::ColumnNameMismatch => "column-name-mismatch",
            Rule::TypeddictKeyMismatch => "typeddict-key-mismatch",
            Rule::TypeddictInheritance => "typeddict-inheritance",
            Rule::NullableMismatch => "nullable-mismatch",
            Rule::JsonShapeMismatch => "json-shape-mismatch",
            Rule::RedundantOptional => "redundant-optional",
            Rule::LiteralMismatch => "literal-mismatch",
            Rule::RedundantLiteral => "redundant-literal",
            Rule::SelectStar => "select-star",
            Rule::SelectStarNoSchema => "select-star-no-schema",
            Rule::StaleRewrite => "stale-rewrite",
            Rule::StaleType => "stale-type",
            Rule::MysqlSqlMarker => "mysql-sql-marker",
            Rule::InlineSqlComment => "inline-sql-comment",
            Rule::FstringSkipped => "fstring-skipped",
            Rule::TstringSkipped => "tstring-skipped",
            Rule::UncheckedQuery => "unchecked-query",
            Rule::UntypedQuery => "untyped-query",
            Rule::UnparsedQuery => "unparsed-query",
            Rule::UnusedIgnore => "unused-ignore",
            Rule::ParamCountMismatch => "param-count-mismatch",
            Rule::ParamTypeMismatch => "param-type-mismatch",
            Rule::Name(rule) => rule.name(),
        }
    }

    /// The rule called `name`, if there is one.
    pub fn parse(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|rule| rule.name() == name)
    }

    /// Severity when `[tool.sqlty.rules]` does not set one.
    pub fn default_severity(self) -> Severity {
        match self {
            Rule::ColumnCountMismatch
            | Rule::ColumnTypeMismatch
            | Rule::TypeddictKeyMismatch
            | Rule::TypeddictInheritance
            | Rule::NullableMismatch
            | Rule::JsonShapeMismatch
            | Rule::LiteralMismatch
            | Rule::ParamCountMismatch
            | Rule::ParamTypeMismatch
            | Rule::SelectStarNoSchema => Severity::Error,
            Rule::ColumnNameMismatch
            | Rule::SelectStar
            | Rule::StaleRewrite
            | Rule::StaleType
            | Rule::MysqlSqlMarker
            | Rule::InlineSqlComment
            | Rule::FstringSkipped
            | Rule::TstringSkipped
            | Rule::UncheckedQuery
            | Rule::UntypedQuery => Severity::Warn,
            Rule::RedundantOptional
            | Rule::RedundantLiteral
            | Rule::UnparsedQuery
            | Rule::UnusedIgnore
            | Rule::Name(_) => Severity::Ignore,
        }
    }
}

/// How loudly a rule reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Ignore,
}

impl Severity {
    /// The severity called `name` (`error`, `warn` or `ignore`), if there is one.
    pub fn parse(name: &str) -> Option<Severity> {
        match name {
            "error" => Some(Severity::Error),
            "warn" => Some(Severity::Warn),
            "ignore" => Some(Severity::Ignore),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Ignore => "ignore",
        }
    }
}

/// A rule name or severity in the overrides that sqlty does not know.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleConfigError {
    #[error("unknown rule: {0}")]
    UnknownRule(String),
    #[error("unknown severity for rule {rule}: {severity} (expected error, warn or ignore)")]
    UnknownSeverity { rule: String, severity: String },
}

/// Rule severity configuration: the defaults plus the user's overrides.
#[derive(Clone, Debug, Default)]
pub struct RuleConfig {
    overrides: HashMap<Rule, Severity>,
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
        let mut parsed = HashMap::with_capacity(entries.len());
        for (rule_name, severity_name) in entries {
            let rule = Rule::parse(&rule_name)
                .ok_or_else(|| RuleConfigError::UnknownRule(rule_name.clone()))?;
            let severity = Severity::parse(&severity_name).ok_or({
                RuleConfigError::UnknownSeverity {
                    rule: rule_name,
                    severity: severity_name,
                }
            })?;
            parsed.insert(rule, severity);
        }
        Ok(RuleConfig { overrides: parsed })
    }

    /// The effective severity of a rule.
    pub fn severity(&self, rule: Rule) -> Severity {
        self.overrides
            .get(&rule)
            .copied()
            .unwrap_or_else(|| rule.default_severity())
    }

    /// Whether a rule reports at all (not `ignore`).
    pub fn is_enabled(&self, rule: Rule) -> bool {
        self.severity(rule) != Severity::Ignore
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_name_round_trips() {
        for rule in Rule::ALL {
            assert_eq!(Rule::parse(rule.name()), Some(rule));
        }
    }

    #[test]
    fn defaults_match_the_documented_table() {
        let config = RuleConfig::default();
        assert_eq!(config.severity(Rule::SelectStarNoSchema), Severity::Error);
        assert_eq!(config.severity(Rule::StaleRewrite), Severity::Warn);
        assert_eq!(config.severity(Rule::UnparsedQuery), Severity::Ignore);
        assert!(!config.is_enabled(Rule::Name(NameRule::UnknownTable)));
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
