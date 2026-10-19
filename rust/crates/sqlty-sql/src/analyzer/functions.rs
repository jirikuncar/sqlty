//! Function return types and aggregate typing.

use std::rc::Rc;

use sqlparser::ast::{BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments};

use super::dialect::SqlDialect;
use super::expr::{from_sql_type, range_element, range_kind};
use super::inference::{function_rules, FunctionRule};
use super::literals::{strip, value_set};
use super::operators::{common_of, mysql_date_add, numeric_rank, operand_type};
use super::query::Analyzer;
use super::scope::{resolve, Scope};
use super::typed::Typed;
use super::{decimal_type, InferError, Inferred};

/// A result type a function rule computes from the arguments (`type = "@series"`, ...);
/// positions count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Returns {
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
    /// MySQL `TIMESTAMPADD(unit, n, d)`: a `DATE` stays a date for day units and coarser,
    /// anything else is a `DATETIME`.
    TimestampAdd,
    /// MySQL `CEIL` and `FLOOR`: an integer for an exact number whose integer part fits a
    /// `BIGINT`, a `DOUBLE` for an approximate one.
    Integral,
    /// `timezone(zone, ts)`, like `ts AT TIME ZONE zone`: a `TIMESTAMP` becomes a
    /// `TIMESTAMPTZ` and back, a time becomes a `TIMETZ`. Without a zone (`timezone(ts)`,
    /// `AT LOCAL`), the last argument is converted.
    AtZone(usize),
    /// `generate_series(start, stop[, step])`: the common type of the bounds, with small
    /// integers widened to `INT` and dates to `TIMESTAMPTZ`, as PostgreSQL resolves it.
    Series,
    /// A string function with a binary overload: `TEXT`, or the type of argument `i` when
    /// it is a binary string (or, in PostgreSQL, a bit string).
    Text(usize),
    /// `random()`: `DOUBLE`; PostgreSQL 17's `random(min, max)`: the type of the bounds, as
    /// for [`Returns::Series`].
    Random,
    /// The range type of a range or multirange argument: `range_merge`, `unnest`.
    Range(usize),
    /// The multirange type of a range or multirange argument: `multirange`, `range_agg`.
    Multirange(usize),
}

use Returns::{Arg, Elem};

/// The types of the first two arguments of a call, where a function rule read them.
pub(super) type RuleArguments = [Option<Typed>; 2];

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
            Returns::Void => Ok(Typed {
                python: Some(Rc::from("None")),
                sql: Some(Rc::from("VOID")),
                values: None,
            }),
            Arg(i) => Ok(strip(arg(i)?, self.dialect)),
            // `LAG(x, 1, default)` can also return the default, of their common type.
            Returns::Same(i) if args.len() >= 3 => {
                let value = strip(arg(i)?, self.dialect);
                let default = arg(args.len() - 1)?;
                Ok(common_of([value.clone(), default], self.dialect).unwrap_or(value))
            }
            Returns::Same(i) => arg(i),
            Returns::FirstKnown => {
                let mut typed_args = Vec::with_capacity(args.len());
                for expr in args {
                    typed_args.push(self.expr(expr, scope)?);
                }
                if let Some(common) = common_of(typed_args.iter().cloned(), self.dialect) {
                    return Ok(common);
                }
                let first = typed_args
                    .iter()
                    .find(|typed| typed.is_known())
                    .cloned()
                    .unwrap_or(Typed::UNKNOWN);
                Ok(if value_set(&first).is_some() {
                    strip(first, self.dialect)
                } else {
                    first
                })
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
            Elem(i) => Ok(self.unnested_typed(arg(i)?.sql())),
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
            Returns::Range(i) | Returns::Multirange(i) => {
                let typed = arg(i)?;
                let suffix = match returns {
                    Returns::Range(_) => "RANGE",
                    _ => "MULTIRANGE",
                };
                match typed.sql().filter(|ty| range_element(ty).is_some()) {
                    Some(ty) => sql(&format!("{}{suffix}", range_kind(ty).unwrap_or_default())),
                    None => Ok(Typed::UNKNOWN),
                }
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
            Returns::TimestampAdd => {
                let day_or_coarser = matches!(
                    args.first(),
                    Some(Expr::Identifier(unit)) if ["DAY", "WEEK", "MONTH", "QUARTER", "YEAR"]
                        .iter()
                        .any(|coarse| unit.value.eq_ignore_ascii_case(coarse))
                );
                match operand_type(&arg(2)?) {
                    Some("DATE") if day_or_coarser => sql("DATE"),
                    Some("DATE" | "DATETIME" | "TIMESTAMP") => sql("DATETIME"),
                    _ => Ok(Typed::UNKNOWN),
                }
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
            Returns::AtZone(i) => Ok(
                match operand_type(&arg(i.min(args.len().saturating_sub(1)))?) {
                    Some("TIMESTAMP") => from_sql_type("TIMESTAMPTZ", self.dialect),
                    Some("TIMESTAMPTZ") => from_sql_type("TIMESTAMP", self.dialect),
                    Some("TIME" | "TIMETZ") => from_sql_type("TIMETZ", self.dialect),
                    _ => Typed::UNKNOWN,
                },
            ),
            Returns::Trunc(i) => {
                let typed = arg(i)?;
                match operand_type(&typed) {
                    Some("DATE") => sql("TIMESTAMPTZ"),
                    Some("TIMESTAMP" | "TIMESTAMPTZ" | "INTERVAL") => Ok(typed),
                    _ => Ok(Typed::UNKNOWN),
                }
            }
            Returns::Text(i) => {
                let typed = arg(i)?;
                match operand_type(&typed) {
                    Some(
                        binary @ ("VARBINARY" | "BINARY" | "BYTEA" | "BLOB" | "TINYBLOB"
                        | "MEDIUMBLOB" | "LONGBLOB"),
                    ) => sql(binary),
                    Some(bits @ ("BIT" | "VARBIT")) if self.dialect == SqlDialect::Postgres => {
                        sql(bits)
                    }
                    _ => sql("TEXT"),
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

    /// The inference rule for a call of the built-in `name` (upper case), with the types of
    /// the first two arguments where a rule reads them.
    pub(super) fn function_rule(
        &self,
        name: &str,
        args: &[&Expr],
        scope: &Scope,
    ) -> Inferred<Option<(&'static FunctionRule, RuleArguments)>> {
        let rules = function_rules(name);
        if rules.is_empty() {
            return Ok(None);
        }
        let typed = |position: usize| -> Inferred<Option<Typed>> {
            if !rules.iter().any(|rule| rule.reads(position)) {
                return Ok(None);
            }
            Ok(Some(match args.get(position) {
                Some(arg) => self.expr(arg, scope)?,
                None => Typed::UNKNOWN,
            }))
        };
        let typed = [typed(0)?, typed(1)?];
        let [first, second] = [0, 1].map(|i| typed[i].as_ref().and_then(operand_type));
        let all = if rules.iter().any(FunctionRule::reads_all) {
            args.iter()
                .map(|arg| self.expr(arg, scope))
                .collect::<Inferred<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let all: Vec<Option<&str>> = all.iter().map(operand_type).collect();
        let found = rules
            .iter()
            .find(|rule| rule.applies(self.dialect, first, second, &all));
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
