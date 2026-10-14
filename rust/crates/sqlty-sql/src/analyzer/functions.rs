//! Function return types and aggregate typing.

use super::*;

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
    (&["AVG"], Returns::Mean),
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
            "CHR",
            "QUOTE_IDENT",
            "QUOTE_LITERAL",
            "QUOTE_NULLABLE",
            "SPLIT_PART",
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
        ],
        Sql("INT"),
    ),
    (
        &[
            "NEXTVAL",
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
    (
        &[
            "RANDOM",
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
    (&["TO_DATE", "MAKE_DATE"], Sql("DATE")),
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
        &["STRING_TO_ARRAY", "REGEXP_SPLIT_TO_ARRAY"],
        Sql("ARRAY<TEXT>"),
    ),
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
            "GREATEST",
            "LEAST",
            "ARRAY_APPEND",
            "ARRAY_CAT",
            "ARRAY_REMOVE",
            "ARRAY_REPLACE",
            "GENERATE_SERIES",
        ],
        Arg(0),
    ),
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
            "CEIL", "CEILING", "FLOOR", "SQRT", "CBRT", "EXP", "LN", "POWER", "SIGN",
        ],
        Returns::Real,
    ),
    (&["ROUND", "TRUNC", "LOG"], Returns::Scaled),
    (&["DATE_TRUNC"], Returns::Trunc(1)),
];

pub(super) fn function_table(
    entries: &'static [(&[&'static str], Returns)],
) -> HashMap<&'static str, Returns> {
    entries
        .iter()
        .flat_map(|(names, returns)| names.iter().map(move |name| (*name, *returns)))
        .collect()
}

pub(super) static FUNCTION_TYPES: LazyLock<HashMap<&'static str, Returns>> =
    LazyLock::new(|| function_table(COMMON_FUNCTIONS));
pub(super) static POSTGRES_FUNCTION_TYPES: LazyLock<HashMap<&'static str, Returns>> =
    LazyLock::new(|| function_table(POSTGRES_FUNCTIONS));

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
    pub(super) fn returns(&self, name: &str) -> Option<Returns> {
        let postgres = (self.dialect == SqlDialect::Postgres)
            .then(|| POSTGRES_FUNCTION_TYPES.get(name))
            .flatten();
        postgres.or_else(|| FUNCTION_TYPES.get(name)).copied()
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
            Returns::Sql(name) => sql(name),
            Returns::Void => Ok(Typed {
                python: Some(Rc::from("None")),
                sql: Some(Rc::from("VOID")),
            }),
            Returns::Arg(i) => Ok(strip(arg(i)?, self.dialect)),
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
            Returns::Elem(i) => Ok(self.element_typed(arg(i)?.sql())),
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
            Returns::Trunc(i) => {
                let typed = arg(i)?;
                match operand_type(&typed) {
                    Some("DATE") => sql("TIMESTAMPTZ"),
                    Some("TIMESTAMP" | "TIMESTAMPTZ" | "INTERVAL") => Ok(typed),
                    _ => Ok(Typed::UNKNOWN),
                }
            }
        }
    }

    /// SUM/MIN/MAX take their argument's type; SUM falls back to `NUMERIC`.
    pub(super) fn aggregate(
        &self,
        name: &str,
        arg: Option<&Expr>,
        scope: &Scope,
    ) -> Inferred<Typed> {
        let mut typed = arg.map_or(Ok(Typed::UNKNOWN), |arg| self.expr(arg, scope))?;
        if name == "SUM" {
            typed = strip(typed, self.dialect);
        }
        let numeric = || {
            Ok(Typed {
                python: Some(Rc::from(decimal_type(self.dialect))),
                sql: Some(Rc::from("NUMERIC")),
            })
        };
        if typed.python.is_some() || name != "SUM" {
            return Ok(if name == "SUM" && self.dialect == SqlDialect::Postgres {
                sum_type(typed, self.dialect)
            } else {
                typed
            });
        }
        if scope.is_empty() {
            return numeric();
        }
        let (table, column) = match arg {
            Some(Expr::Identifier(ident)) => (scope[0].1.clone(), &ident.value),
            Some(Expr::CompoundIdentifier(parts)) if parts.len() >= 2 => (
                resolve(&parts[parts.len() - 2].value, scope),
                &parts[parts.len() - 1].value,
            ),
            _ => return numeric(),
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

/// PostgreSQL's `SUM` widens: `BIGINT` for `SMALLINT`/`INT`, `NUMERIC` for `BIGINT`/`NUMERIC`.
pub(super) fn sum_type(typed: Typed, dialect: SqlDialect) -> Typed {
    let widened = match operand_type(&typed) {
        Some("SMALLINT" | "INT") => "BIGINT",
        Some("BIGINT" | "DECIMAL" | "NUMERIC") => "DECIMAL",
        Some("FLOAT") => "FLOAT",
        Some("DOUBLE") => "DOUBLE",
        _ => return typed,
    };
    from_sql_type(widened, dialect)
}
