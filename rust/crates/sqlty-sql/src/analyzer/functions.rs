//! Function return types and aggregate typing.

use std::rc::Rc;

use sqlparser::ast::{BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments};

use super::dialect::SqlDialect;
use super::expr::{from_sql_type, range_element};
use super::inference::{function_rules, FunctionRule};
use super::literals::{strip, value_set};
use super::operators::{common_of, mysql_date_add, numeric_rank, operand_type};
use super::query::Analyzer;
use super::registry::lookup;
use super::scope::{resolve, Scope};
use super::typed::Typed;
use super::{decimal_type, InferError, Inferred};

/// How a function's result type follows from its arguments (indexes count from 0).
#[derive(Clone, Copy)]
pub(super) enum Returns {
    /// A fixed canonical SQL type.
    Sql(&'static str),
    /// `void`: Python `None`.
    Void,
    /// The type of an argument, whatever value it computes.
    Arg(usize),
    /// An argument's type and values: the function returns one of its argument's values.
    Same(usize),
    /// The element type of an array argument.
    Elem(usize),
    /// An array of an argument's type.
    ArrayOf(usize),
    /// The first argument of known type, skipping `NULL` and placeholders.
    FirstKnown,
    /// [`Returns::FirstKnown`] of the arguments from this one on: `IF(cond, a, b)`.
    Branches(usize),
    /// MySQL `STR_TO_DATE(s, format)`: a `DATE`, `TIME` or `DATETIME` by the parts the
    /// literal format at this argument reads.
    StrToDate(usize),
    /// `lower`/`upper`: the element type of a range argument, else `TEXT`.
    Bound,
    /// `AVG` and friends: `DOUBLE` for floats, `INTERVAL` for intervals, else `NUMERIC`.
    Mean,
    /// `sqrt`, `sign`, ...: `DOUBLE` for floats and integers (PostgreSQL prefers
    /// `double precision`), `NUMERIC` for numerics.
    Real,
    /// `round`, `trunc`, `log`: [`Returns::Real`], except `NUMERIC` when called with a
    /// scale or base (a second argument).
    Scaled,
    /// `date_trunc`: a timestamp argument's type, `TIMESTAMPTZ` for a date.
    Trunc(usize),
    /// MySQL `DATE_ADD(d, INTERVAL n unit)` and friends: like `d + INTERVAL n unit`; a
    /// number of days keeps a date a date.
    DateAdd,
    /// MySQL `CEIL` and `FLOOR`: an integer for an exact number whose integer part fits a
    /// `BIGINT`, a `DOUBLE` for an approximate one.
    Integral,
    /// `timezone(zone, ts)`, like `ts AT TIME ZONE zone`: a `TIMESTAMP` becomes a
    /// `TIMESTAMPTZ` and back, a time becomes a `TIMETZ`.
    AtZone(usize),
    /// `generate_series(start, stop[, step])`: the common type of the bounds, with small
    /// integers widened to `INT` and dates to `TIMESTAMPTZ`, as PostgreSQL resolves it.
    Series,
    /// `random()`: `DOUBLE`; PostgreSQL 17's `random(min, max)`: the type of the bounds, as
    /// for [`Returns::Series`].
    Random,
}

use Returns::{Arg, Elem, Sql};

/// Functions of every dialect, by upper-case name (COUNT, SUM, MIN, MAX and COALESCE are
/// special-cased).
pub(super) const COMMON_FUNCTIONS: &[(&[&str], Returns)] = &[
    (
        &[
            "LENGTH",
            "POSITION",
            "CHAR_LENGTH",
            "BIT_LENGTH",
            "OCTET_LENGTH",
            "NTILE",
        ],
        Sql("INT"),
    ),
    (&["ROW_NUMBER", "RANK", "DENSE_RANK"], Sql("BIGINT")),
    (&["COALESCE"], Returns::FirstKnown),
    (
        &[
            "STDDEV",
            "STDDEV_POP",
            "STDDEV_SAMP",
            "VARIANCE",
            "VAR_POP",
            "VAR_SAMP",
        ],
        Sql("DOUBLE"),
    ),
    (
        &[
            "UPPER",
            "LOWER",
            "TRIM",
            "CONCAT",
            "SUBSTR",
            "SUBSTRING",
            "REPLACE",
            "STRING_AGG",
            "GROUP_CONCAT",
        ],
        Sql("TEXT"),
    ),
    (&["NOW", "CURRENT_TIMESTAMP"], Sql("TIMESTAMPTZ")),
    (&["CURRENT_DATE"], Sql("DATE")),
    (&["CURRENT_TIME"], Sql("TIMETZ")),
    (
        &[
            "JSON_BUILD_OBJECT",
            "TO_JSON",
            "JSON_AGG",
            "JSON_OBJECT_AGG",
        ],
        Sql("JSON"),
    ),
    (
        &[
            "JSONB_BUILD_OBJECT",
            "TO_JSONB",
            "JSONB_AGG",
            "JSONB_OBJECT_AGG",
        ],
        Sql("JSONB"),
    ),
];

/// PostgreSQL built-ins, from the "Functions and Operators" chapter of the manual; in
/// PostgreSQL they take precedence over [`COMMON_FUNCTIONS`].
pub(super) const POSTGRES_FUNCTIONS: &[(&[&str], Returns)] = &[
    (
        &[
            "CURRENT_SETTING",
            "SET_CONFIG",
            "LEFT",
            "RIGHT",
            "LPAD",
            "RPAD",
            "LTRIM",
            "RTRIM",
            "BTRIM",
            "INITCAP",
            "REPEAT",
            "REVERSE",
            "TRANSLATE",
            "OVERLAY",
            "CONCAT_WS",
            "FORMAT",
            "MD5",
            "TO_CHAR",
            "TO_HEX",
            "TO_BIN",
            "TO_OCT",
            "CHR",
            "QUOTE_IDENT",
            "QUOTE_LITERAL",
            "QUOTE_NULLABLE",
            "SPLIT_PART",
            "NORMALIZE",
            "REGEXP_REPLACE",
            "ENCODE",
            "CONVERT_FROM",
            "ARRAY_TO_STRING",
            "PG_SIZE_PRETTY",
            "VERSION",
            "CURRENT_DATABASE",
            "CURRENT_SCHEMA",
            "CURRENT_USER",
            "SESSION_USER",
            "USER",
            "PG_TYPEOF",
            "JSONB_TYPEOF",
            "JSON_TYPEOF",
            "JSONB_PRETTY",
            "JSONB_EXTRACT_PATH_TEXT",
            "JSON_EXTRACT_PATH_TEXT",
            "TS_HEADLINE",
            "JSONB_ARRAY_ELEMENTS_TEXT",
            "JSON_ARRAY_ELEMENTS_TEXT",
            "ARRAY_DIMS",
            "HOST",
            "ABBREV",
            "TEXT",
            "TIMEOFDAY",
            "OBJ_DESCRIPTION",
            "COL_DESCRIPTION",
            "FORMAT_TYPE",
            "UNISTR",
            "JSON_OBJECT_KEYS",
            "JSONB_OBJECT_KEYS",
            "REGEXP_SPLIT_TO_TABLE",
            "STRING_TO_TABLE",
        ],
        Sql("TEXT"),
    ),
    (
        &[
            "ASCII",
            "STRPOS",
            "ARRAY_LENGTH",
            "ARRAY_NDIMS",
            "ARRAY_POSITION",
            "CARDINALITY",
            "JSONB_ARRAY_LENGTH",
            "JSON_ARRAY_LENGTH",
            "WIDTH_BUCKET",
            "PG_BACKEND_PID",
            "NUM_NONNULLS",
            "NUM_NULLS",
            "ARRAY_LOWER",
            "ARRAY_UPPER",
            "GET_BYTE",
            "GET_BIT",
            "MASKLEN",
            "FAMILY",
            "INET_CLIENT_PORT",
            "INET_SERVER_PORT",
            "GROUPING",
            "PG_COLUMN_SIZE",
            "REGEXP_COUNT",
            "SCALE",
        ],
        Sql("INT"),
    ),
    (
        &[
            "NEXTVAL",
            "REGR_COUNT",
            "CURRVAL",
            "LASTVAL",
            "SETVAL",
            "TXID_CURRENT",
            "PG_DATABASE_SIZE",
            "PG_TOTAL_RELATION_SIZE",
            "PG_RELATION_SIZE",
            "PG_TABLE_SIZE",
            "PG_INDEXES_SIZE",
        ],
        Sql("BIGINT"),
    ),
    (&["RANDOM"], Returns::Random),
    (
        &[
            "DATE_PART",
            "PI",
            "DEGREES",
            "RADIANS",
            "SIN",
            "COS",
            "TAN",
            "ASIN",
            "ACOS",
            "ATAN",
            "ATAN2",
            "PERCENT_RANK",
            "CUME_DIST",
            "CORR",
            "COVAR_POP",
            "COVAR_SAMP",
            "REGR_SLOPE",
            "REGR_INTERCEPT",
            "REGR_R2",
            "REGR_AVGX",
            "REGR_AVGY",
            "REGR_SXX",
            "REGR_SYY",
            "REGR_SXY",
        ],
        Sql("DOUBLE"),
    ),
    (&["TS_RANK", "TS_RANK_CD"], Sql("FLOAT")),
    (&["TO_NUMBER"], Sql("DECIMAL")),
    (
        &[
            "STARTS_WITH",
            "REGEXP_LIKE",
            "PG_TRY_ADVISORY_LOCK",
            "PG_TRY_ADVISORY_XACT_LOCK",
            "PG_TRY_ADVISORY_LOCK_SHARED",
            "PG_TRY_ADVISORY_XACT_LOCK_SHARED",
            "PG_ADVISORY_UNLOCK",
            "PG_ADVISORY_UNLOCK_SHARED",
            "PG_IS_IN_RECOVERY",
            "PG_TERMINATE_BACKEND",
            "PG_CANCEL_BACKEND",
            "ISFINITE",
            "BOOL_AND",
            "BOOL_OR",
            "EVERY",
            "LOWER_INF",
            "UPPER_INF",
            "ISEMPTY",
            "JSONB_PATH_EXISTS",
            "JSON_VALID",
            "JSONB_EXISTS",
            "JSONB_EXISTS_ANY",
            "JSONB_EXISTS_ALL",
            "HAS_TABLE_PRIVILEGE",
            "HAS_SCHEMA_PRIVILEGE",
            "HAS_COLUMN_PRIVILEGE",
            "HAS_DATABASE_PRIVILEGE",
            "HAS_FUNCTION_PRIVILEGE",
            "HAS_SEQUENCE_PRIVILEGE",
            "PG_HAS_ROLE",
        ],
        Sql("BOOLEAN"),
    ),
    (
        &[
            "PG_ADVISORY_LOCK",
            "PG_ADVISORY_XACT_LOCK",
            "PG_ADVISORY_LOCK_SHARED",
            "PG_ADVISORY_XACT_LOCK_SHARED",
            "PG_ADVISORY_UNLOCK_ALL",
            "PG_NOTIFY",
            "PG_SLEEP",
            "SETSEED",
        ],
        Returns::Void,
    ),
    (
        &[
            "TRANSACTION_TIMESTAMP",
            "STATEMENT_TIMESTAMP",
            "CLOCK_TIMESTAMP",
            "TO_TIMESTAMP",
            "MAKE_TIMESTAMPTZ",
        ],
        Sql("TIMESTAMPTZ"),
    ),
    (&["LOCALTIMESTAMP", "MAKE_TIMESTAMP"], Sql("TIMESTAMP")),
    (&["TO_DATE", "MAKE_DATE", "DATE"], Sql("DATE")),
    (&["LOCALTIME", "MAKE_TIME"], Sql("TIME")),
    (
        &[
            "AGE",
            "JUSTIFY_DAYS",
            "JUSTIFY_HOURS",
            "JUSTIFY_INTERVAL",
            "MAKE_INTERVAL",
        ],
        Sql("INTERVAL"),
    ),
    (
        &["GEN_RANDOM_UUID", "UUID_GENERATE_V4", "UUID_GENERATE_V1"],
        Sql("UUID"),
    ),
    (&["UUID_EXTRACT_VERSION"], Sql("SMALLINT")),
    (
        &["UUID_EXTRACT_TIMESTAMP", "DATE_ADD", "DATE_SUBTRACT"],
        Sql("TIMESTAMPTZ"),
    ),
    (&["RANDOM_NORMAL"], Sql("DOUBLE")),
    (
        &[
            "SHA224",
            "SHA256",
            "SHA384",
            "SHA512",
            "DECODE",
            "GEN_RANDOM_BYTES",
            "CONVERT_TO",
            "DIGEST",
        ],
        Sql("VARBINARY"),
    ),
    (
        &[
            "STRING_TO_ARRAY",
            "REGEXP_SPLIT_TO_ARRAY",
            "REGEXP_MATCH",
            "REGEXP_MATCHES",
        ],
        Sql("ARRAY<TEXT>"),
    ),
    (&["ARRAY_POSITIONS"], Sql("ARRAY<INT>")),
    (&["NETWORK", "INET_MERGE"], Sql("CIDR")),
    (
        &[
            "BROADCAST",
            "HOSTMASK",
            "NETMASK",
            "INET_CLIENT_ADDR",
            "INET_SERVER_ADDR",
        ],
        Sql("INET"),
    ),
    (&["SET_MASKLEN"], Arg(0)),
    (&["DATE_BIN"], Arg(1)),
    (&["TIMEZONE"], Returns::AtZone(1)),
    (&["ARRAY_FILL"], Returns::ArrayOf(0)),
    (&["BIT_AND", "BIT_OR", "BIT_XOR"], Arg(0)),
    (
        &[
            "ROW_TO_JSON",
            "JSON_BUILD_ARRAY",
            "JSON_OBJECT",
            "JSON_STRIP_NULLS",
            "JSON_EXTRACT_PATH",
            "JSON_ARRAY_ELEMENTS",
        ],
        Sql("JSON"),
    ),
    (
        &[
            "JSONB_BUILD_ARRAY",
            "JSONB_OBJECT",
            "JSONB_STRIP_NULLS",
            "JSONB_SET",
            "JSONB_INSERT",
            "JSONB_EXTRACT_PATH",
            "JSONB_ARRAY_ELEMENTS",
            "JSONB_PATH_QUERY",
            "JSONB_PATH_QUERY_FIRST",
        ],
        Sql("JSONB"),
    ),
    (&["TO_TSVECTOR", "SETWEIGHT"], Sql("TSVECTOR")),
    (
        &[
            "TO_TSQUERY",
            "PLAINTO_TSQUERY",
            "PHRASETO_TSQUERY",
            "WEBSEARCH_TO_TSQUERY",
        ],
        Sql("TSQUERY"),
    ),
    (
        &[
            "ROW",
            "JSON_EACH",
            "JSONB_EACH",
            "JSON_EACH_TEXT",
            "JSONB_EACH_TEXT",
        ],
        Sql("RECORD"),
    ),
    (&["PG_GET_USERBYID"], Sql("NAME")),
    (&["FACTORIAL"], Sql("DECIMAL")),
    (&["RANGE_MERGE"], Arg(0)),
    (&["INT4RANGE"], Sql("INT4RANGE")),
    (&["INT8RANGE"], Sql("INT8RANGE")),
    (&["NUMRANGE"], Sql("NUMRANGE")),
    (&["TSRANGE"], Sql("TSRANGE")),
    (&["TSTZRANGE"], Sql("TSTZRANGE")),
    (&["DATERANGE"], Sql("DATERANGE")),
    (
        &[
            "ABS",
            "MOD",
            "ARRAY_APPEND",
            "ARRAY_CAT",
            "ARRAY_REMOVE",
            "ARRAY_REPLACE",
            "TRIM_ARRAY",
            "ARRAY_SAMPLE",
            "ARRAY_SHUFFLE",
        ],
        Arg(0),
    ),
    (&["GENERATE_SERIES"], Returns::Series),
    (
        &[
            "NULLIF",
            "FIRST_VALUE",
            "LAST_VALUE",
            "NTH_VALUE",
            "LAG",
            "LEAD",
            "ANY_VALUE",
        ],
        Returns::Same(0),
    ),
    (&["GREATEST", "LEAST"], Returns::FirstKnown),
    (&["ARRAY_PREPEND"], Arg(1)),
    (&["UNNEST"], Elem(0)),
    (&["ARRAY_AGG"], Returns::ArrayOf(0)),
    (&["LOWER", "UPPER"], Returns::Bound),
    (
        &[
            "STDDEV",
            "STDDEV_POP",
            "STDDEV_SAMP",
            "VARIANCE",
            "VAR_POP",
            "VAR_SAMP",
        ],
        Returns::Mean,
    ),
    (
        &[
            "CEIL", "CEILING", "FLOOR", "SQRT", "CBRT", "EXP", "LN", "LOG10", "POWER", "SIGN",
        ],
        Returns::Real,
    ),
    (&["ROUND", "TRUNC", "LOG"], Returns::Scaled),
    (&["DATE_TRUNC"], Returns::Trunc(1)),
];

/// The upper-case name of a function, without its schema.
pub(super) fn function_name(func: &sqlparser::ast::Function) -> String {
    func.name
        .0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_uppercase())
        .unwrap_or_default()
}

/// The leading unnamed expression arguments of a call (`*` and named arguments end the list).
pub(super) fn positional_args(args: &FunctionArguments) -> Vec<&Expr> {
    match args {
        FunctionArguments::List(list) => list
            .args
            .iter()
            .map_while(|arg| match arg {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => Some(expr),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl<'a> Analyzer<'a> {
    /// How the result type of the built-in function `name` (upper case) follows from its arguments.
    pub(super) fn returns(&self, name: &str) -> Option<Returns> {
        lookup(name)?.returns(self.dialect)
    }

    /// Result type of a call to a function that [`Returns`] describes, given its arguments.
    pub(super) fn function_type(
        &self,
        returns: Returns,
        args: &[&Expr],
        scope: &Scope,
    ) -> Inferred<Typed> {
        let arg = |i: usize| match args.get(i) {
            Some(arg) => self.expr(arg, scope),
            None => Ok(Typed::UNKNOWN),
        };
        let sql = |name: &str| Ok(from_sql_type(name, self.dialect));
        match returns {
            Sql(name) => sql(name),
            Returns::Void => Ok(Typed {
                python: Some(Rc::from("None")),
                sql: Some(Rc::from("VOID")),
                values: None,
            }),
            Arg(i) => Ok(strip(arg(i)?, self.dialect)),
            // `LAG(x, 1, 'default')` can also return the default.
            Returns::Same(i) if args.len() >= 3 => Ok(strip(arg(i)?, self.dialect)),
            Returns::Same(i) => arg(i),
            Returns::FirstKnown => {
                let mut typed_args = Vec::with_capacity(args.len());
                for expr in args {
                    typed_args.push(self.expr(expr, scope)?);
                }
                if self.dialect == SqlDialect::Postgres {
                    if let Some(common) = common_of(typed_args.iter().cloned(), self.dialect) {
                        return Ok(common);
                    }
                }
                let first = typed_args
                    .iter()
                    .find(|typed| typed.is_known())
                    .cloned()
                    .unwrap_or(Typed::UNKNOWN);
                // `COALESCE(checked_column, 'other')` takes any of the arguments' values.
                if value_set(&first).is_some() {
                    return Ok(common_of(typed_args, self.dialect)
                        .unwrap_or_else(|| strip(first, self.dialect)));
                }
                Ok(first)
            }
            Returns::Branches(i) => self.function_type(
                Returns::FirstKnown,
                args.get(i..).unwrap_or_default(),
                scope,
            ),
            Returns::StrToDate(i) => Ok(match args.get(i) {
                Some(Expr::Value(value)) => match value.value.clone().into_string() {
                    Some(format) => from_sql_type(str_to_date_type(&format), self.dialect),
                    None => Typed::UNKNOWN,
                },
                _ => Typed::UNKNOWN,
            }),
            Elem(i) => Ok(self.element_typed(arg(i)?.sql())),
            Returns::ArrayOf(i) => {
                let typed = arg(i)?;
                let elem = typed
                    .sql()
                    .or_else(|| operand_type(&typed))
                    .map(String::from);
                Ok(elem.map_or(Typed::UNKNOWN, |elem| {
                    from_sql_type(&format!("ARRAY<{elem}>"), self.dialect)
                }))
            }
            Returns::Bound => {
                let typed = arg(0)?;
                match typed.sql().and_then(range_element) {
                    Some(elem) => sql(elem),
                    None if typed.python.is_some() => sql("TEXT"),
                    None => Ok(Typed::UNKNOWN),
                }
            }
            Returns::Mean => {
                let typed = arg(0)?;
                match operand_type(&typed) {
                    Some("DOUBLE" | "FLOAT") => sql("DOUBLE"),
                    Some("INTERVAL") => sql("INTERVAL"),
                    Some(_) => sql("DECIMAL"),
                    None => Ok(Typed::UNKNOWN),
                }
            }
            Returns::Real | Returns::Scaled => {
                let typed = arg(0)?;
                match operand_type(&typed) {
                    _ if matches!(returns, Returns::Scaled) && args.len() > 1 => sql("DECIMAL"),
                    Some("DOUBLE" | "FLOAT") => sql("DOUBLE"),
                    Some(ty) if numeric_rank(ty).is_some_and(|rank| rank <= 2) => sql("DOUBLE"),
                    Some("DECIMAL" | "NUMERIC") => sql("DECIMAL"),
                    _ => Ok(Typed::UNKNOWN),
                }
            }
            Returns::DateAdd => {
                let date = arg(0)?;
                Ok(match args.get(1) {
                    Some(interval @ Expr::Interval(_)) => mysql_date_add(
                        &BinaryOperator::Plus,
                        (args[0], &date),
                        (interval, &Typed::UNKNOWN),
                    )
                    .map_or(Typed::UNKNOWN, |ty| from_sql_type(ty, self.dialect)),
                    Some(_) => match operand_type(&date) {
                        Some("DATE") => sql("DATE")?,
                        Some("DATETIME" | "TIMESTAMP" | "TIMESTAMPTZ") => sql("DATETIME")?,
                        _ => Typed::UNKNOWN,
                    },
                    None => Typed::UNKNOWN,
                })
            }
            Returns::Integral => {
                let typed = arg(0)?;
                let sql_type = typed.sql().unwrap_or_default();
                match operand_type(&typed) {
                    Some("DOUBLE" | "FLOAT" | "REAL") => sql("DOUBLE"),
                    Some("DECIMAL" | "NUMERIC") if decimal_integer_digits(sql_type) > 18 => {
                        Ok(strip(typed, self.dialect))
                    }
                    Some(_) => sql("BIGINT"),
                    None => Ok(Typed::UNKNOWN),
                }
            }
            Returns::AtZone(i) => Ok(match operand_type(&arg(i)?) {
                Some("TIMESTAMP") => from_sql_type("TIMESTAMPTZ", self.dialect),
                Some("TIMESTAMPTZ") => from_sql_type("TIMESTAMP", self.dialect),
                Some("TIME" | "TIMETZ") => from_sql_type("TIMETZ", self.dialect),
                _ => Typed::UNKNOWN,
            }),
            Returns::Trunc(i) => {
                let typed = arg(i)?;
                match operand_type(&typed) {
                    Some("DATE") => sql("TIMESTAMPTZ"),
                    Some("TIMESTAMP" | "TIMESTAMPTZ" | "INTERVAL") => Ok(typed),
                    _ => Ok(Typed::UNKNOWN),
                }
            }
            Returns::Random if args.is_empty() => sql("DOUBLE"),
            Returns::Random | Returns::Series => {
                let bounds = [arg(0)?, arg(1)?];
                match common_of(bounds, self.dialect)
                    .as_ref()
                    .and_then(operand_type)
                {
                    Some("SMALLINT" | "INT") => sql("INT"),
                    Some("BIGINT") => sql("BIGINT"),
                    Some("DECIMAL" | "NUMERIC") => sql("DECIMAL"),
                    Some("TIMESTAMP") => sql("TIMESTAMP"),
                    Some("TIMESTAMPTZ" | "DATE") => sql("TIMESTAMPTZ"),
                    _ => Ok(Typed::UNKNOWN),
                }
            }
        }
    }

    /// The inference rule for a call of the built-in `name` (upper case), with the first
    /// argument's type when a rule read it.
    pub(super) fn function_rule(
        &self,
        name: &str,
        arg: Option<&Expr>,
        scope: &Scope,
    ) -> Inferred<Option<(&'static FunctionRule, Option<Typed>)>> {
        let rules = function_rules(name);
        if rules.is_empty() {
            return Ok(None);
        }
        let typed = if rules.iter().any(FunctionRule::reads_argument) {
            Some(arg.map_or(Ok(Typed::UNKNOWN), |arg| self.expr(arg, scope))?)
        } else {
            None
        };
        let argument = typed.as_ref().and_then(operand_type);
        let found = rules
            .iter()
            .find(|rule| rule.applies(self.dialect, argument));
        Ok(found.map(|rule| (rule, typed.clone())))
    }

    /// `SUM` of what no inference rule types: its argument's own type (a value set dropped),
    /// else `NUMERIC`, or an error for a column the schema table it reads lacks.
    pub(super) fn sum(&self, typed: Typed, arg: Option<&Expr>, scope: &Scope) -> Inferred<Typed> {
        let typed = strip(typed, self.dialect);
        if typed.python.is_some() {
            return Ok(typed);
        }
        let numeric = || {
            Ok(Typed {
                python: Some(Rc::from(decimal_type(self.dialect))),
                sql: Some(Rc::from("NUMERIC")),
                values: None,
            })
        };
        if scope.is_empty() {
            return numeric();
        }
        let (table, column) = match arg {
            Some(Expr::Identifier(ident)) => (scope[0].table.clone(), &ident.value),
            Some(Expr::CompoundIdentifier(parts)) if parts.len() >= 2 => (
                resolve(&parts[parts.len() - 2].value, scope),
                &parts[parts.len() - 1].value,
            ),
            _ => return numeric(),
        };
        let Some(table) = table.named().map(str::to_string) else {
            return numeric();
        };
        if self.schema.table(&table).is_some() && self.virtual_columns(&table).is_none() {
            return Err(InferError::SumColumnNotInSchema {
                column: column.clone(),
                table,
            });
        }
        numeric()
    }
}

/// The type MySQL's `STR_TO_DATE` returns for `format`: `DATETIME` when it reads both date
/// and time parts, else `DATE` or `TIME`.
fn str_to_date_type(format: &str) -> &'static str {
    let specifiers: Vec<char> = format
        .split('%')
        .skip(1)
        .filter_map(|part| part.chars().next())
        .collect();
    let time = specifiers.iter().any(|c| "fHhIiklprSsT".contains(*c));
    let date = specifiers
        .iter()
        .any(|c| "abcDdeJjMmUuVvWwXxYy".contains(*c));
    match (date, time) {
        (true, true) => "DATETIME",
        (false, true) => "TIME",
        _ => "DATE",
    }
}

/// The digits before the point of a `DECIMAL(p, s)`: `p - s`; MySQL's bare `DECIMAL` is
/// `DECIMAL(10, 0)`.
fn decimal_integer_digits(sql_type: &str) -> u32 {
    let numbers: Vec<u32> = sql_type
        .split(['(', ',', ')'])
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    match numbers.as_slice() {
        [precision, scale, ..] => precision.saturating_sub(*scale),
        [precision] => *precision,
        [] => 10,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sqlparser::ast::Function;

    use super::super::test_support::{expr, reports};
    use super::*;

    fn call(sql: &str) -> Function {
        match expr(sql) {
            Expr::Function(func) => func,
            other => panic!("not a call: {other:?}"),
        }
    }

    #[test]
    fn function_names_are_upper_case_without_the_schema() {
        assert_eq!(function_name(&call("lower(x)")), "LOWER");
        assert_eq!(function_name(&call("pg_catalog.Lower(x)")), "LOWER");
        assert_eq!(function_name(&call("\"MyFn\"(x)")), "MYFN");
    }

    #[test]
    fn positional_args_stop_at_the_first_star_or_named_argument() {
        let args = |sql: &str| {
            positional_args(&call(sql).args)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(args("f(a, b + 1)"), ["a", "b + 1"]);
        assert!(args("count(*)").is_empty());
        assert_eq!(args("f(a, b => 1, c)"), ["a"]);
        assert!(args("now()").is_empty());
    }

    #[test]
    fn positional_args_of_a_function_without_a_list_are_empty() {
        let Expr::Function(func) = expr("current_timestamp") else {
            panic!("expected a function");
        };
        assert!(positional_args(&func.args).is_empty());
    }

    #[test]
    fn function_tables_have_upper_case_names() {
        for table in [COMMON_FUNCTIONS, POSTGRES_FUNCTIONS] {
            for name in table.iter().flat_map(|(names, _)| names.iter()) {
                assert_eq!(*name, name.to_uppercase(), "{name} is not upper case");
            }
        }
    }

    #[test]
    fn function_tables_list_a_name_once() {
        for table in [COMMON_FUNCTIONS, POSTGRES_FUNCTIONS] {
            let mut seen = HashSet::new();
            for name in table.iter().flat_map(|(names, _)| names.iter()) {
                assert!(seen.insert(*name), "{name} is listed twice");
            }
        }
    }

    const DDL: &str = "CREATE TABLE t (i INT NOT NULL, s SMALLINT, b BIGINT NOT NULL, \
                       n NUMERIC(10, 2) NOT NULL, f DOUBLE PRECISION, txt TEXT, \
                       d DATE, ts TIMESTAMP NOT NULL, tz TIMESTAMPTZ, iv INTERVAL, \
                       arr INT[] NOT NULL, tags TEXT[], r INT4RANGE, j JSONB)";

    #[test]
    fn sum_widens_in_postgres() {
        let widen = |sql: &str| {
            let declared = if sql == "DOUBLE" {
                "DOUBLE PRECISION"
            } else {
                sql
            };
            let ddl = format!("CREATE TABLE t (c {declared})");
            let schema = crate::analyzer::load_schema(&ddl, "postgres");
            crate::analyzer::analyze_select("SELECT SUM(c) AS s FROM t", "postgres", Some(&schema))
                .ok()
                .flatten()
                .and_then(|columns| columns[0].sql_type.clone())
        };
        assert_eq!(widen("SMALLINT").as_deref(), Some("BIGINT"));
        assert_eq!(widen("INT").as_deref(), Some("BIGINT"));
        assert_eq!(widen("BIGINT").as_deref(), Some("DECIMAL"));
        assert_eq!(widen("DECIMAL(10, 2)").as_deref(), Some("DECIMAL"));
        assert_eq!(widen("REAL").as_deref(), Some("FLOAT"));
        assert_eq!(widen("DOUBLE").as_deref(), Some("DOUBLE"));
        assert_eq!(widen("INTERVAL").as_deref(), Some("INTERVAL"));
        assert_eq!(widen("TEXT").as_deref(), Some("TEXT"));
    }

    #[test]
    fn aggregates_in_postgres() {
        let queries = [
            "SELECT count(*) AS a, count(txt) AS b, count(DISTINCT i) AS c FROM t",
            "SELECT sum(i) AS a, sum(s) AS b, sum(b) AS c, sum(n) AS d, sum(f) AS e FROM t",
            "SELECT min(i) AS a, max(txt) AS b, min(d) AS c, max(ts) AS d FROM t",
            "SELECT avg(i) AS a, avg(f) AS b, avg(n) AS c, avg(iv) AS d FROM t",
            "SELECT sum(i) FILTER (WHERE i > 1) AS a FROM t",
            "SELECT array_agg(i) AS a, string_agg(txt, ',') AS b, bool_and(i > 1) AS c FROM t",
            "SELECT sum(nope) FROM t",
            "SELECT sum(i::BIGINT) AS a, sum(unknown_fn(i)) AS b FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn return_types_follow_the_argument() {
        let queries = [
            "SELECT abs(i) AS a, abs(n) AS b, abs(f) AS c, abs(s) AS d FROM t",
            "SELECT round(n) AS a, round(n, 1) AS b, round(f) AS c, round(i) AS d FROM t",
            "SELECT sqrt(i) AS a, sqrt(n) AS b, ceil(f) AS c, sign(n) AS d FROM t",
            "SELECT length(txt) AS a, lower(txt) AS b, concat(txt, i) AS c, substr(txt, 1) AS d FROM t",
            "SELECT coalesce(s, 1) AS a, coalesce(i, 1.5) AS b, coalesce(txt, 'x') AS c, coalesce(NULL, NULL) AS d FROM t",
            "SELECT greatest(i, b) AS a, least(i, 1) AS b, nullif(i, 0) AS c FROM t",
            "SELECT now() AS a, current_date AS b, date_trunc('day', tz) AS c, date_trunc('day', d) AS d FROM t",
            "SELECT extract(year FROM ts) AS a, age(ts) AS b, date_part('day', ts) AS c FROM t",
            "SELECT lag(i) OVER () AS a, lag(i, 1, 0) OVER () AS b, row_number() OVER () AS c FROM t",
            "SELECT lower(r) AS a, upper(r) AS b, array_length(arr, 1) AS c, unnest(arr) AS d FROM t",
            "SELECT arr[1] AS a, tags[1] AS b, array_append(arr, 1) AS c, array_agg(txt) AS d FROM t GROUP BY txt, arr, tags",
            "SELECT gen_random_uuid() AS a, md5(txt) AS b, random() AS c, pg_sleep(1) AS d FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn mysql_sum_of_an_integer_is_decimal() {
        let report = super::super::test_support::report(
            "SELECT sum(i) AS total FROM t",
            "mysql",
            Some("CREATE TABLE t (i INT NOT NULL)"),
        );
        assert!(
            report.starts_with("total: decimal.Decimal | DECIMAL"),
            "{report}"
        );
    }

    #[test]
    fn unknown_functions_are_untyped_and_explained() {
        let queries = [
            "SELECT definitely_not_a_function(i) AS a FROM t",
            "SELECT public.also_missing(i) AS a FROM t",
            "SELECT now() AS ok, mystery() AS bad FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn function_availability_differs_by_dialect() {
        let queries = [
            "SELECT ifnull(i, 0) AS a, concat_ws(',', txt, txt) AS b, if(i > 1, 1, 0) AS c FROM t",
            "SELECT date_format(ts, '%Y') AS a, curdate() AS b, group_concat(txt) AS c FROM t",
            "SELECT avg(i) AS a, sum(i) AS b, round(n, 1) AS c, stddev(i) AS d FROM t",
        ];
        let ddl = "CREATE TABLE t (i INT NOT NULL, txt VARCHAR(10), ts DATETIME, \
                   n DECIMAL(10, 2) NOT NULL)";
        let mysql = reports(&queries, "mysql", Some(ddl));
        let postgres = reports(&queries, "postgres", None);
        insta::assert_snapshot!(format!("mysql:\n{mysql}\npostgres:\n{postgres}"));
    }
}
