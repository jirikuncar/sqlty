//! What the analyzer knows about each built-in function besides its rules: whether it is an
//! aggregate and the types its parameters take, folded into one table of [`FunctionInfo`]
//! so a call is classified with a single lookup of its upper-case name.
//!
//! A function's result type and nullability are inference rules (`rules/`); behaviour that
//! depends on the arguments (`MOD` by zero, `LAG`'s default) stays in
//! `Analyzer::function_nullability`.

use std::collections::HashMap;
use std::sync::LazyLock;

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
    pub(super) behavior: Behavior,
    /// All arguments share one type, so a placeholder takes the type of its siblings.
    pub(super) same_type: bool,
    /// The SQL type of each leading argument; an empty name leaves it untyped.
    pub(super) signature: Option<&'static [&'static str]>,
}

impl FunctionInfo {
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
    for name in AGGREGATES {
        entry(&mut registry, name).behavior = Behavior::Aggregate;
    }
    for name in NULL_ON_FEW_ROWS {
        entry(&mut registry, name).behavior = Behavior::NullOnFewRows;
    }
    for name in ALWAYS_VALUED_AGGREGATES {
        entry(&mut registry, name).behavior = Behavior::AlwaysValued;
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
    "RANGE_AGG",
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
    "ST_COLLECT",
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_AGG_STRICT",
    "JSONB_AGG_STRICT",
    "JSON_OBJECT_AGG_STRICT",
    "JSONB_OBJECT_AGG_STRICT",
    "JSON_OBJECT_AGG_UNIQUE",
    "JSONB_OBJECT_AGG_UNIQUE",
    "JSON_OBJECT_AGG_UNIQUE_STRICT",
    "JSONB_OBJECT_AGG_UNIQUE_STRICT",
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
    "MEDIAN",
    "PERCENTILE",
    "JSONB_GROUP_ARRAY",
    "JSONB_GROUP_OBJECT",
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
    "MEDIAN",
    "PERCENTILE",
];

/// Aggregates whose group of one or more rows always gives a value, whatever the rows hold.
const ALWAYS_VALUED_AGGREGATES: &[&str] = &[
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_AGG_STRICT",
    "JSONB_AGG_STRICT",
    "JSON_OBJECT_AGG_STRICT",
    "JSONB_OBJECT_AGG_STRICT",
    "JSON_OBJECT_AGG_UNIQUE",
    "JSONB_OBJECT_AGG_UNIQUE",
    "JSON_OBJECT_AGG_UNIQUE_STRICT",
    "JSONB_OBJECT_AGG_UNIQUE_STRICT",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "TOTAL",
    "JSON_GROUP_ARRAY",
    "JSON_GROUP_OBJECT",
    "JSONB_GROUP_ARRAY",
    "JSONB_GROUP_OBJECT",
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
    fn the_first_signature_of_a_name_wins() {
        assert_eq!(
            lookup("SPLIT_PART").and_then(|f| f.signature),
            Some(&["TEXT", "TEXT", "INT"][..])
        );
        assert!(lookup("COALESCE").is_some_and(|f| f.same_type));
        assert!(lookup("NOT_A_FUNCTION").is_none());
    }
}
