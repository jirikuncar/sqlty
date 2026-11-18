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
    /// The row type has a different number of fields than the query has columns.
    ColumnCountMismatch,
    /// A row type field cannot hold its column's values.
    ColumnTypeMismatch,
    /// A `NamedTuple` field name differs from its column name.
    ColumnNameMismatch,
    /// The columns do not fit the fields of a TypedDict, dataclass or model.
    TypeddictKeyMismatch,
    /// A TypedDict subclass breaks a closed or `extra_items` base (PEP 728).
    TypeddictInheritance,
    /// A field is not optional but its column can be NULL.
    NullableMismatch,
    /// A field cannot hold the JSON its column builds.
    JsonShapeMismatch,
    /// A field is optional but its column cannot be NULL.
    RedundantOptional,
    /// A `Literal` field does not cover the values of its column.
    LiteralMismatch,
    /// A `Literal` field lists values its column never takes.
    RedundantLiteral,
    /// `SELECT *`, which the fixer expands.
    SelectStar,
    /// `SELECT *` with no schema to expand it.
    SelectStarNoSchema,
    /// A `--sql` string that is not wrapped in `SQL[T](...)`.
    StaleRewrite,
    /// A `SQL[tuple[...]]` wider than the inferred row type: it admits every row.
    StaleType,
    /// A `SQL[tuple[...]]` that does not admit every row the query returns.
    NarrowType,
    /// A `--sql` marker under MySQL, which rejects it.
    MysqlSqlMarker,
    /// `"--sql SELECT ..."`: the whole query is a SQL comment.
    InlineSqlComment,
    /// An f-string with `--sql`, which cannot be rewritten.
    FstringSkipped,
    /// A t-string with `--sql`, which cannot be rewritten.
    TstringSkipped,
    /// A `--sql` query that is not plain string literals.
    UncheckedQuery,
    /// A helper call whose query is not a string literal.
    DynamicQuery,
    /// A `SQL[UNTYPED]` query.
    UntypedQuery,
    /// A query that does not parse, so it cannot be checked.
    UnparsedQuery,
    /// A `# sqlty: ignore` comment that suppresses nothing.
    UnusedIgnore,
    /// A call passes a different number of parameters than its query has.
    ParamCountMismatch,
    /// A call passes a key its query has no placeholder for.
    ExtraParam,
    /// A call passes an argument its query's parameter cannot take.
    ParamTypeMismatch,
    /// The parameter type `P` of `SQL[R, P]` does not fit the parameters of its query.
    ParamsTypeMismatch,
    /// A finding of sqruff's lint rules in a query.
    Sqruff,
    /// A query names a database the project does not configure.
    UnknownDatabase,
    /// sqlty crashed checking the file; nothing else of it is reported.
    InternalError,
    /// A rule explaining why a column is untyped.
    Name(NameRule),
}

impl Rule {
    /// Every rule.
    pub const ALL: [Rule; 47] = [
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
        Rule::NarrowType,
        Rule::MysqlSqlMarker,
        Rule::InlineSqlComment,
        Rule::FstringSkipped,
        Rule::TstringSkipped,
        Rule::UncheckedQuery,
        Rule::DynamicQuery,
        Rule::UntypedQuery,
        Rule::UnparsedQuery,
        Rule::UnusedIgnore,
        Rule::ParamCountMismatch,
        Rule::ExtraParam,
        Rule::ParamTypeMismatch,
        Rule::ParamsTypeMismatch,
        Rule::Sqruff,
        Rule::UnknownDatabase,
        Rule::InternalError,
        Rule::Name(NameRule::UnknownTable),
        Rule::Name(NameRule::UnknownColumn),
        Rule::Name(NameRule::AmbiguousColumn),
        Rule::Name(NameRule::DuplicateAlias),
        Rule::Name(NameRule::UnknownFunction),
        Rule::Name(NameRule::UnknownType),
        Rule::Name(NameRule::UnsupportedSyntax),
        Rule::Name(NameRule::InsertMissingColumn),
        Rule::Name(NameRule::InsertValueCount),
        Rule::Name(NameRule::UngroupedColumn),
        Rule::Name(NameRule::FanoutAggregate),
        Rule::Name(NameRule::ChasmJoin),
        Rule::Name(NameRule::UnprovenJoin),
        Rule::Name(NameRule::EntityMismatch),
        Rule::Name(NameRule::ForeignKeyMismatch),
        Rule::Name(NameRule::RejectedQuery),
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
            Rule::NarrowType => "narrow-type",
            Rule::MysqlSqlMarker => "mysql-sql-marker",
            Rule::InlineSqlComment => "inline-sql-comment",
            Rule::FstringSkipped => "fstring-skipped",
            Rule::TstringSkipped => "tstring-skipped",
            Rule::UncheckedQuery => "unchecked-query",
            Rule::DynamicQuery => "dynamic-query",
            Rule::UntypedQuery => "untyped-query",
            Rule::UnparsedQuery => "unparsed-query",
            Rule::UnusedIgnore => "unused-ignore",
            Rule::ParamCountMismatch => "param-count-mismatch",
            Rule::ExtraParam => "extra-param",
            Rule::ParamTypeMismatch => "param-type-mismatch",
            Rule::ParamsTypeMismatch => "params-type-mismatch",
            Rule::Sqruff => "sqruff",
            Rule::UnknownDatabase => "unknown-database",
            Rule::InternalError => "internal-error",
            Rule::Name(rule) => rule.name(),
        }
    }

    /// Every rule, in the order `sqlty rule` lists them.
    pub const LISTED: [Rule; 47] = [
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
        Rule::NarrowType,
        Rule::MysqlSqlMarker,
        Rule::InlineSqlComment,
        Rule::FstringSkipped,
        Rule::TstringSkipped,
        Rule::UncheckedQuery,
        Rule::DynamicQuery,
        Rule::UntypedQuery,
        Rule::UnparsedQuery,
        Rule::Name(NameRule::UnknownTable),
        Rule::Name(NameRule::UnknownColumn),
        Rule::Name(NameRule::AmbiguousColumn),
        Rule::Name(NameRule::DuplicateAlias),
        Rule::Name(NameRule::UnknownFunction),
        Rule::Name(NameRule::UnknownType),
        Rule::Name(NameRule::UnsupportedSyntax),
        Rule::Name(NameRule::InsertMissingColumn),
        Rule::Name(NameRule::InsertValueCount),
        Rule::Name(NameRule::UngroupedColumn),
        Rule::Name(NameRule::FanoutAggregate),
        Rule::Name(NameRule::ChasmJoin),
        Rule::Name(NameRule::UnprovenJoin),
        Rule::Name(NameRule::EntityMismatch),
        Rule::Name(NameRule::ForeignKeyMismatch),
        Rule::Name(NameRule::RejectedQuery),
        Rule::ParamCountMismatch,
        Rule::ExtraParam,
        Rule::ParamTypeMismatch,
        Rule::Sqruff,
        Rule::UnknownDatabase,
        Rule::ParamsTypeMismatch,
        Rule::UnusedIgnore,
        Rule::InternalError,
    ];

    /// What the rule reports, in a line.
    pub fn description(self) -> &'static str {
        match self {
            Rule::ColumnCountMismatch => "Annotated type has different number of fields than query columns",
            Rule::ColumnTypeMismatch => "Annotated type field has incompatible type with query column",
            Rule::ColumnNameMismatch => "Annotated NamedTuple field name doesn't match query column alias",
            Rule::TypeddictKeyMismatch => "Query columns don't fit the fields of a TypedDict, dataclass or Pydantic model",
            Rule::TypeddictInheritance => "TypedDict subclass changes openness or adds keys a closed or extra_items base forbids (PEP 728)",
            Rule::NullableMismatch => "Row type field is not optional but its query column can be NULL",
            Rule::JsonShapeMismatch => "Row type field cannot hold the JSON a query column builds",
            Rule::RedundantOptional => "Row type field is optional but its query column cannot be NULL",
            Rule::LiteralMismatch => "Row type field is a Literal that does not cover the values its column can take",
            Rule::RedundantLiteral => "Row type field is a Literal that lists values its column can never take",
            Rule::SelectStar => "SELECT * used \u{2014} expand to explicit column list",
            Rule::SelectStarNoSchema => "SELECT * used but no schema available to expand",
            Rule::StaleRewrite => "Source has --sql strings that aren't wrapped in SQL[T]",
            Rule::StaleType => "SQL[tuple[...]] is wider than the rows the query returns (drift)",
            Rule::NarrowType => "SQL[tuple[...]] doesn't admit every row the query returns",
            Rule::MysqlSqlMarker => "MySQL rejects the \"--sql\" marker; use \"-- sql\"",
            Rule::InlineSqlComment => "--sql followed by space makes entire query a SQL comment; use \"--sql\\nSELECT ...\" or run sqlty check --fix",
            Rule::FstringSkipped => "f-string with --sql prefix skipped (cannot be statically rewritten)",
            Rule::TstringSkipped => "t-string with --sql prefix skipped (a field changes the SQL in a way sqlty cannot follow)",
            Rule::UncheckedQuery => "--sql query is not plain string literals (raw, bytes, an expression), so it is not checked",
            Rule::DynamicQuery => "helper query that is not a string literal (a variable, a call), so it is not checked",
            Rule::UntypedQuery => "SQL[UNTYPED] query has no inferable row type; the message names why",
            Rule::UnparsedQuery => "--sql query, SQL[T] query or Annotated helper query that does not parse",
            Rule::Name(NameRule::UnknownTable) => "Query reads a table that the schema does not define",
            Rule::Name(NameRule::UnknownColumn) => "Query reads a column that its table does not have",
            Rule::Name(NameRule::AmbiguousColumn) => "Unqualified column exists in more than one table of the query",
            Rule::Name(NameRule::DuplicateAlias) => "Two tables of a FROM clause are read by the same name",
            Rule::Name(NameRule::UnknownFunction) => "Function result is untyped because sqlty does not know the function",
            Rule::Name(NameRule::UnknownType) => "Cast or column type has no Python type",
            Rule::Name(NameRule::UnsupportedSyntax) => "Query parses, but a construct in it is not typed, so the row type is not trusted",
            Rule::Name(NameRule::InsertMissingColumn) => "INSERT leaves out a NOT NULL column that has no default",
            Rule::Name(NameRule::InsertValueCount) => "INSERT gives a different number of values than the columns it writes",
            Rule::Name(NameRule::UngroupedColumn) => "Grouped query selects a column it neither groups by nor aggregates",
            Rule::Name(NameRule::FanoutAggregate) => "Aggregate over rows a one-to-many join repeats",
            Rule::Name(NameRule::ChasmJoin) => "Aggregating query joins one table one-to-many to two others",
            Rule::Name(NameRule::UnprovenJoin) => "Join in an aggregating query that matches no primary or unique key",
            Rule::Name(NameRule::EntityMismatch) => "Comparison between columns holding keys of different tables",
            Rule::Name(NameRule::ForeignKeyMismatch) => "Foreign key column compared with a column it does not reference",
            Rule::Name(NameRule::RejectedQuery) => "Query the database rejects for a reason no other rule names",
            Rule::ParamCountMismatch => "A call passes a different number of parameters than its query has placeholders, or misses a name",
            Rule::ExtraParam => "A call passes a key its query has no placeholder for",
            Rule::ParamTypeMismatch => "A call passes an argument whose type can't be the type its query parameter takes",
            Rule::Sqruff => "Finding of sqruff's core lint rules in a query",
            Rule::UnknownDatabase => "Query marker names a database the project does not configure",
            Rule::InternalError => "sqlty crashed checking the file",
            Rule::ParamsTypeMismatch => "The parameter type `P` of `SQL[R, P]` doesn't fit the parameters of its query",
            Rule::UnusedIgnore => "# sqlty: ignore comment, or rule in one, that suppresses no diagnostic",
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
            | Rule::ParamsTypeMismatch
            | Rule::NarrowType
            | Rule::SelectStarNoSchema
            | Rule::UnknownDatabase
            | Rule::InternalError
            | Rule::Name(
                NameRule::UnknownTable
                | NameRule::UnknownColumn
                | NameRule::AmbiguousColumn
                | NameRule::DuplicateAlias
                | NameRule::InsertMissingColumn
                | NameRule::InsertValueCount
                | NameRule::UngroupedColumn
                | NameRule::RejectedQuery,
            ) => Severity::Error,
            Rule::ColumnNameMismatch
            | Rule::SelectStar
            | Rule::StaleRewrite
            | Rule::StaleType
            | Rule::MysqlSqlMarker
            | Rule::InlineSqlComment
            | Rule::FstringSkipped
            | Rule::TstringSkipped
            | Rule::UncheckedQuery
            | Rule::UntypedQuery
            | Rule::UnparsedQuery
            | Rule::UnusedIgnore
            | Rule::ExtraParam
            | Rule::Name(
                NameRule::UnknownFunction
                | NameRule::UnknownType
                | NameRule::UnsupportedSyntax
                | NameRule::FanoutAggregate
                | NameRule::ChasmJoin
                | NameRule::ForeignKeyMismatch,
            ) => Severity::Warn,
            Rule::RedundantOptional
            | Rule::RedundantLiteral
            | Rule::DynamicQuery
            | Rule::Sqruff
            | Rule::Name(NameRule::UnprovenJoin | NameRule::EntityMismatch) => Severity::Ignore,
        }
    }
}

/// How loudly a rule reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Reported as an error: the check fails.
    Error,
    /// Reported as a warning.
    Warn,
    /// Not reported.
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

    /// The name [`Severity::parse`] reads back: `error`, `warn` or `ignore`.
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

    /// This configuration with every rule but `rules` ignored.
    pub(crate) fn only(&self, rules: &[Rule]) -> RuleConfig {
        RuleConfig {
            overrides: Rule::ALL
                .into_iter()
                .map(|rule| {
                    let severity = if rules.contains(&rule) {
                        self.severity(rule)
                    } else {
                        Severity::Ignore
                    };
                    (rule, severity)
                })
                .collect(),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

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
        assert_eq!(
            config.severity(Rule::Name(NameRule::UnknownTable)),
            Severity::Error
        );
        assert_eq!(
            config.severity(Rule::Name(NameRule::UnknownFunction)),
            Severity::Warn
        );
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
