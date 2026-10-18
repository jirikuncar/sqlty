//! What the analyzer knows about each built-in function, in one place.
//!
//! A function's result type ([`Returns`]), how it behaves with NULLs, and the types its
//! parameters take used to live in separate lists searched by name. They are folded once
//! into a table of [`FunctionInfo`], so a call is classified with a single lookup of its
//! upper-case name, and a new function is a row in the list that describes it.
//!
//! Dialect-specific behaviour that depends on the arguments (`MOD` by zero, `CONCAT` under
//! MySQL, ...) stays in code, in `Analyzer::function_nullability`.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::dialect_functions::{
    MYSQL_ALWAYS_NULLABLE, MYSQL_FUNCTIONS, SQLITE_ALWAYS_NULLABLE, SQLITE_FUNCTIONS,
};
use super::functions::{Returns, COMMON_FUNCTIONS, POSTGRES_FUNCTIONS};
use super::SqlDialect;

/// How a function treats NULLs and rows, apart from its result type.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Behavior {
    /// A scalar function that is NULL for a NULL argument unless the code says otherwise.
    #[default]
    Scalar,
    /// An aggregate: over no rows it gives NULL (except `COUNT`, which is not listed).
    Aggregate,
    /// An aggregate that also gives NULL for a group with too few rows to measure.
    NullOnFewRows,
    /// An aggregate whose group of one or more rows always gives a value.
    AlwaysValued,
}

/// Everything known about one function name.
#[derive(Clone, Copy, Default)]
pub(super) struct FunctionInfo {
    returns: Option<Returns>,
    /// PostgreSQL's own definition, which takes precedence over `returns` there.
    postgres_returns: Option<Returns>,
    /// MySQL's, likewise.
    mysql_returns: Option<Returns>,
    /// SQLite's, likewise.
    sqlite_returns: Option<Returns>,
    pub(super) behavior: Behavior,
    /// Can be NULL for non-null arguments.
    always_nullable: bool,
    /// Can be NULL for non-null arguments in MySQL.
    mysql_nullable: bool,
    /// Can be NULL for non-null arguments in SQLite.
    sqlite_nullable: bool,
    /// Never NULL.
    pub(super) never_null: bool,
    /// Looks through NULL arguments (see [`LOOKS_THROUGH_NULLS`]).
    looks_through_nulls: bool,
    /// All arguments share one type, so a placeholder takes the type of its siblings.
    pub(super) same_type: bool,
    /// The SQL type of each leading argument; an empty name leaves it untyped.
    pub(super) signature: Option<&'static [&'static str]>,
}

impl FunctionInfo {
    /// The result type rule in `dialect`, when the analyzer has one.
    pub(super) fn returns(&self, dialect: SqlDialect) -> Option<Returns> {
        match dialect {
            SqlDialect::Postgres => self.postgres_returns.or(self.returns),
            SqlDialect::MySql => self.mysql_returns.or(self.returns),
            SqlDialect::Sqlite => self.sqlite_returns.or(self.returns),
            _ => self.returns,
        }
    }

    /// Whether the result can be NULL for non-null arguments in `dialect`.
    pub(super) fn always_nullable(&self, dialect: SqlDialect) -> bool {
        self.always_nullable
            || (dialect == SqlDialect::MySql && self.mysql_nullable)
            || (dialect == SqlDialect::Sqlite && self.sqlite_nullable)
    }

    /// Whether the result is NULL for a NULL argument, as far as narrowing goes: a function
    /// with a known result type, other than the ones that look through NULLs.
    pub(super) fn is_strict(&self, dialect: SqlDialect) -> bool {
        self.returns(dialect).is_some()
            && self.behavior != Behavior::AlwaysValued
            && !self.looks_through_nulls
    }

    pub(super) fn is_aggregate(&self) -> bool {
        self.behavior != Behavior::Scalar
    }
}

/// The function named `name` (upper case, without its schema), when any list knows it.
pub(super) fn lookup(name: &str) -> Option<&'static FunctionInfo> {
    REGISTRY.get(name)
}

static REGISTRY: LazyLock<HashMap<&'static str, FunctionInfo>> = LazyLock::new(|| {
    let mut registry: HashMap<&'static str, FunctionInfo> = HashMap::new();
    fn entry<'m>(
        registry: &'m mut HashMap<&'static str, FunctionInfo>,
        name: &'static str,
    ) -> &'m mut FunctionInfo {
        registry.entry(name).or_default()
    }
    // Later rows of a list replace earlier ones.
    for (names, returns) in COMMON_FUNCTIONS {
        for name in *names {
            entry(&mut registry, name).returns = Some(*returns);
        }
    }
    for (names, returns) in POSTGRES_FUNCTIONS {
        for name in *names {
            entry(&mut registry, name).postgres_returns = Some(*returns);
        }
    }
    for (names, returns) in MYSQL_FUNCTIONS {
        for name in *names {
            entry(&mut registry, name).mysql_returns = Some(*returns);
        }
    }
    for (names, returns) in SQLITE_FUNCTIONS {
        for name in *names {
            entry(&mut registry, name).sqlite_returns = Some(*returns);
        }
    }
    for name in MYSQL_ALWAYS_NULLABLE {
        entry(&mut registry, name).mysql_nullable = true;
    }
    for name in SQLITE_ALWAYS_NULLABLE {
        entry(&mut registry, name).sqlite_nullable = true;
    }
    for name in AGGREGATES {
        entry(&mut registry, name).behavior = Behavior::Aggregate;
    }
    for name in NULL_ON_FEW_ROWS {
        entry(&mut registry, name).behavior = Behavior::NullOnFewRows;
    }
    for name in ALWAYS_VALUED_AGGREGATES {
        entry(&mut registry, name).behavior = Behavior::AlwaysValued;
    }
    for name in ALWAYS_NULLABLE {
        entry(&mut registry, name).always_nullable = true;
    }
    for name in NEVER_NULL {
        entry(&mut registry, name).never_null = true;
    }
    for name in LOOKS_THROUGH_NULLS {
        entry(&mut registry, name).looks_through_nulls = true;
    }
    for name in SAME_TYPE {
        entry(&mut registry, name).same_type = true;
    }
    // The first row that lists a name gives its signature.
    for (names, types) in SIGNATURES {
        for name in *names {
            entry(&mut registry, name).signature.get_or_insert(types);
        }
    }
    registry
});

/// Aggregate functions, whose result over no rows is NULL (except `COUNT`).
const AGGREGATES: &[&str] = &[
    "SUM",
    "RANGE_INTERSECT_AGG",
    "AVG",
    "MIN",
    "MAX",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "STD",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "BOOL_AND",
    "BOOL_OR",
    "EVERY",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
    "ANY_VALUE",
    "STRING_AGG",
    "GROUP_CONCAT",
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "XMLAGG",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
    "MODE",
    "CORR",
    "COVAR_POP",
    "COVAR_SAMP",
    "REGR_SLOPE",
    "REGR_INTERCEPT",
    "REGR_R2",
    "REGR_AVGX",
    "REGR_AVGY",
    "REGR_SXX",
    "REGR_SXY",
    "REGR_SYY",
    "TOTAL",
    "JSON_GROUP_ARRAY",
    "JSON_GROUP_OBJECT",
];

/// Aggregates that give NULL for a group of rows too: too few non-null rows for a sample
/// statistic, a constant input for a correlation, or only NULLs to take a percentile of.
const NULL_ON_FEW_ROWS: &[&str] = &[
    "STDDEV",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_SAMP",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
    "MODE",
    "CORR",
    "COVAR_POP",
    "COVAR_SAMP",
    "REGR_SLOPE",
    "REGR_INTERCEPT",
    "REGR_R2",
    "REGR_AVGX",
    "REGR_AVGY",
    "REGR_SXX",
    "REGR_SXY",
    "REGR_SYY",
];

/// Aggregates whose group of one or more rows always gives a value, whatever the rows hold.
const ALWAYS_VALUED_AGGREGATES: &[&str] = &[
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "XMLAGG",
    "TOTAL",
    "JSON_GROUP_ARRAY",
    "JSON_GROUP_OBJECT",
];

/// Functions that can return NULL for non-null arguments.
const ALWAYS_NULLABLE: &[&str] = &[
    "NULLIF",
    "ARRAY_DIMS",
    "ARRAY_LOWER",
    "ARRAY_UPPER",
    "INET_CLIENT_ADDR",
    "INET_CLIENT_PORT",
    "INET_SERVER_ADDR",
    "INET_SERVER_PORT",
    "REGEXP_MATCH",
    "REGEXP_SUBSTR",
    "ARRAY_LENGTH",
    "ARRAY_UPPER",
    "ARRAY_LOWER",
    "ARRAY_POSITION",
    "JSON_EXTRACT",
    "JSON_EXTRACT_PATH",
    "JSON_EXTRACT_PATH_TEXT",
    "JSONB_EXTRACT_PATH",
    "JSONB_EXTRACT_PATH_TEXT",
    "JSONB_PATH_QUERY_FIRST",
    "JSONB_PATH_QUERY_FIRST_TZ",
    // NULL when the path's first result is not a boolean.
    "JSONB_PATH_MATCH",
    "JSONB_PATH_MATCH_TZ",
    "JSON_VALUE",
    "JSON_QUERY",
    "NTH_VALUE",
    // An array can hold NULL elements.
    "UNNEST",
];

/// Functions that look through NULL arguments, so a condition on their result says nothing
/// about the arguments: a known built-in other than these is NULL for a NULL argument.
const LOOKS_THROUGH_NULLS: &[&str] = &[
    "COALESCE",
    "IFNULL",
    "NVL",
    "CONCAT",
    "CONCAT_WS",
    "GREATEST",
    "LEAST",
    "IF",
    "IIF",
    "COUNT",
    "REGR_COUNT",
    "NULLIF",
    "NVL2",
    "ISNULL",
    "TYPEOF",
    "FIELD",
    "QUOTE",
    "QUOTE_NULLABLE",
    "FORMAT",
    "NUM_NULLS",
    "NUM_NONNULLS",
    "JSON_BUILD_OBJECT",
    "JSONB_BUILD_OBJECT",
    "JSON_BUILD_ARRAY",
    "JSONB_BUILD_ARRAY",
    "JSON_OBJECT",
    "JSON_ARRAY",
    "ROW",
    "ARRAY_APPEND",
    "ARRAY_PREPEND",
    "ARRAY_CAT",
    "ARRAY_TO_STRING",
];

/// Window and other functions that never return NULL.
const NEVER_NULL: &[&str] = &[
    "GROUPING",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "NTILE",
    "PERCENT_RANK",
    "CUME_DIST",
    "NOW",
    "CURRENT_TIMESTAMP",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "RANDOM",
    "RAND",
    "GEN_RANDOM_UUID",
    "UUID",
    "UUID_GENERATE_V4",
    "JSON_BUILD_OBJECT",
    "JSONB_BUILD_OBJECT",
    "JSON_BUILD_ARRAY",
    "JSONB_BUILD_ARRAY",
    "NUM_NULLS",
    "NUM_NONNULLS",
    "JSON_OBJECT",
    "JSON_ARRAY",
    "ROW",
    // `typeof(NULL)` is `'null'`, `quote(NULL)` is `'NULL'`, `FIELD(NULL, ...)` is 0.
    "TYPEOF",
    "QUOTE",
    "FIELD",
];

/// Positions of text and integer arguments of functions with a known signature.
const SIGNATURES: &[(&[&str], &[&str])] = &[
    (
        &[
            "LOWER",
            "UPPER",
            "TRIM",
            "LTRIM",
            "RTRIM",
            "BTRIM",
            "INITCAP",
            "LENGTH",
            "CHAR_LENGTH",
            "REVERSE",
            "MD5",
            "ASCII",
        ],
        &["TEXT"],
    ),
    (&["REPLACE", "TRANSLATE"], &["TEXT", "TEXT", "TEXT"]),
    (&["STRPOS", "STARTS_WITH"], &["TEXT", "TEXT"]),
    (&["SPLIT_PART"], &["TEXT", "TEXT", "INT"]),
    (&["LEFT", "RIGHT", "REPEAT"], &["TEXT", "INT"]),
    (&["LPAD", "RPAD"], &["TEXT", "INT", "TEXT"]),
    (&["SUBSTR"], &["TEXT", "INT", "INT"]),
    (&["DATE_TRUNC"], &["TEXT"]),
    (&["TO_CHAR"], &["", "TEXT"]),
    (&["PG_NOTIFY"], &["TEXT", "TEXT"]),
    (
        &[
            "PG_ADVISORY_LOCK",
            "PG_ADVISORY_LOCK_SHARED",
            "PG_ADVISORY_UNLOCK",
            "PG_ADVISORY_UNLOCK_SHARED",
            "PG_ADVISORY_XACT_LOCK",
            "PG_ADVISORY_XACT_LOCK_SHARED",
            "PG_TRY_ADVISORY_LOCK",
            "PG_TRY_ADVISORY_LOCK_SHARED",
            "PG_TRY_ADVISORY_XACT_LOCK",
            "PG_TRY_ADVISORY_XACT_LOCK_SHARED",
        ],
        &["BIGINT"],
    ),
];

/// Functions whose arguments share one type.
const SAME_TYPE: &[&str] = &["COALESCE", "NULLIF", "GREATEST", "LEAST", "IFNULL"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_refinements_are_aggregates() {
        for name in NULL_ON_FEW_ROWS.iter().chain(ALWAYS_VALUED_AGGREGATES) {
            assert!(AGGREGATES.contains(name), "{name} is not in AGGREGATES");
        }
    }

    #[test]
    fn a_name_has_one_aggregate_behavior() {
        for name in ALWAYS_VALUED_AGGREGATES {
            assert!(!NULL_ON_FEW_ROWS.contains(name), "{name} is in both lists");
        }
    }

    #[test]
    fn postgres_definitions_take_precedence() {
        let info = lookup("STDDEV").expect("STDDEV is known");
        assert!(matches!(
            info.returns(SqlDialect::MySql),
            Some(Returns::Sql("DOUBLE"))
        ));
        assert!(matches!(
            info.returns(SqlDialect::Postgres),
            Some(Returns::Mean)
        ));
    }

    #[test]
    fn the_first_signature_of_a_name_wins() {
        assert_eq!(
            lookup("SPLIT_PART").and_then(|f| f.signature),
            Some(&["TEXT", "TEXT", "INT"][..])
        );
        assert!(lookup("COALESCE").is_some_and(|f| f.same_type));
        assert!(lookup("NOT_A_FUNCTION").is_none());
    }
}
