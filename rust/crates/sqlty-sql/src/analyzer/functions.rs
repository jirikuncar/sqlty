//! Function return types and aggregate typing.

use std::rc::Rc;

use sqlparser::ast::{
    BinaryOperator, DateTimeField, Expr, FunctionArg, FunctionArgExpr, FunctionArgOperator,
    FunctionArguments, Value,
};

use super::dialect::SqlDialect;
use super::expr::{from_sql_type, range_element, range_kind};
use super::inference::{function_rules, operand, FunctionRule};
use super::literals::{strip, value_set};
use super::operators::{
    common_of, is_text, mysql_date_add, mysql_date_shift, numeric_rank, operand_type,
};
use super::query::Analyzer;
use super::scope::{resolve, Scope};
use super::typed::Typed;
use super::{decimal_type, Inferred};

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
    /// DuckDB `date_part(part, x)`: a `DOUBLE` for the literal part at this argument `epoch`
    /// or `julian`, else a `BIGINT`.
    DatePart(usize),
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

/// The arguments of a call written with a list: none for `CURRENT_DATE` or `f(SELECT ...)`.
pub(super) fn arg_list(args: &FunctionArguments) -> &[FunctionArg] {
    match args {
        FunctionArguments::List(list) => &list.args,
        _ => &[],
    }
}

/// The leading unnamed expression arguments of a call (`*` and named arguments end the list).
pub(super) fn positional_args(args: &FunctionArguments) -> Vec<&Expr> {
    match args {
        FunctionArguments::List(list) => list
            .args
            .iter()
            .map_while(|arg| match arg {
                // PostgreSQL's `xmlforest(x AS name)` names an element, and `xmlparse(DOCUMENT x)`
                // sets a mode: the value is still the argument.
                FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::Named { expr, .. })) => {
                    Some(&**expr)
                }
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr))
                | FunctionArg::Named {
                    arg: FunctionArgExpr::Expr(expr),
                    operator: FunctionArgOperator::Space,
                    ..
                } => Some(expr),
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
        self.function_type_of(returns, args, &[None, None], scope)
    }

    /// [`Self::function_type`] given the types of the first two arguments already inferred,
    /// which are not inferred again.
    pub(super) fn function_type_of(
        &self,
        returns: Returns,
        args: &[&Expr],
        known: &RuleArguments,
        scope: &Scope,
    ) -> Inferred<Typed> {
        let arg = |i: usize| match (known.get(i), args.get(i)) {
            (Some(Some(typed)), _) => Ok(typed.clone()),
            (_, Some(arg)) => self.expr(arg, scope),
            (_, None) => Ok(Typed::UNKNOWN),
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
                // An argument of unknown type, a placeholder too, can hold any value of the
                // common type.
                let opaque = args
                    .iter()
                    .zip(&typed_args)
                    .any(|(expr, typed)| !typed.is_known() && !valueless(expr));
                if let Some(common) = common_of(typed_args.iter().cloned(), self.dialect) {
                    return Ok(if opaque {
                        strip(common, self.dialect)
                    } else {
                        common
                    });
                }
                if self.reject_unmatched("argument", &typed_args) {
                    return Ok(Typed::UNKNOWN);
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
            Returns::DatePart(i) => Ok(match args.get(i) {
                Some(Expr::Value(value)) => match value.value.clone().into_string() {
                    Some(part) => from_sql_type(date_part_type(&part), self.dialect),
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
                Ok(elem.map_or(Typed::UNKNOWN, |elem| self.array_of(&elem)))
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
                    // `ADDDATE(date, days)`.
                    Some(_) => mysql_date_shift(&date, true, false)
                        .map_or(Typed::UNKNOWN, |ty| from_sql_type(ty, self.dialect)),
                    None => Typed::UNKNOWN,
                })
            }
            Returns::TimestampAdd => {
                let unit_in = |units: &[&str]| {
                    matches!(
                        args.first(),
                        Some(Expr::Identifier(unit))
                            if units.iter().any(|known| unit.value.eq_ignore_ascii_case(known))
                    )
                };
                let whole_days = unit_in(&["DAY", "WEEK", "MONTH", "QUARTER", "YEAR"]);
                let time_only = unit_in(&["HOUR", "MINUTE", "SECOND", "MICROSECOND"]);
                Ok(mysql_date_shift(&arg(2)?, whole_days, time_only)
                    .map_or(Typed::UNKNOWN, |ty| from_sql_type(ty, self.dialect)))
            }
            Returns::Integral => {
                let typed = arg(0)?;
                let sql_type = typed.sql().unwrap_or_default();
                match operand_type(&typed) {
                    Some("DOUBLE" | "FLOAT" | "REAL") => sql("DOUBLE"),
                    Some(ty)
                        if matches!(ty, "DECIMAL" | "NUMERIC")
                            && decimal_integer_digits(sql_type) > 18 =>
                    {
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
        let [first, second] = [0, 1].map(|i| operand(typed[i].as_ref().and_then(rule_operand)));
        let mut all = Vec::new();
        if rules.iter().any(FunctionRule::reads_all) {
            for arg in args {
                all.push(operand(rule_operand(&self.expr(arg, scope)?)));
            }
        }
        let found = rules
            .iter()
            .find(|rule| rule.applies(self.dialect, first, second, &all));
        Ok(found.map(|rule| (rule, typed.clone())))
    }

    /// `SUM` of what no inference rule types: its argument's own type (a value set dropped),
    /// untyped for a column the schema table it reads lacks (`unknown-column` says why), else
    /// `NUMERIC`.
    pub(super) fn sum(&self, typed: Typed, arg: Option<&Expr>, scope: &Scope) -> Inferred<Typed> {
        let typed = strip(typed, self.dialect);
        if typed.python.is_some() {
            return Ok(typed);
        }
        let numeric = Typed {
            python: Some(Rc::from(decimal_type(self.dialect))),
            sql: Some(Rc::from("NUMERIC")),
            values: None,
        };
        let table = match arg {
            _ if scope.is_empty() => return Ok(numeric),
            Some(Expr::Identifier(_)) => scope[0].table.clone(),
            Some(Expr::CompoundIdentifier(parts)) if parts.len() >= 2 => {
                resolve(&parts[parts.len() - 2].value, scope)
            }
            _ => return Ok(numeric),
        };
        let missing = table.named().is_some_and(|table| {
            self.schema.table(table).is_some() && self.virtual_columns(table).is_none()
        });
        Ok(if missing { Typed::default() } else { numeric })
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

/// An argument's SQL type as function rules match it: a string literal is `TEXT`.
fn rule_operand(typed: &Typed) -> Option<&str> {
    operand_type(typed).or_else(|| is_text(typed).then_some("TEXT"))
}

/// The SQL type `EXTRACT(field FROM x)` gives, `None` when sqlty does not know it.
///
/// PostgreSQL 14+ returns `numeric` for every field (`generic` follows it); MySQL an
/// integer; DuckDB a `BIGINT`, a `DOUBLE` for `epoch` and `julian`; ClickHouse the type of
/// the `toYear`-style function the field reads.
pub(super) fn extract_type(dialect: SqlDialect, field: &DateTimeField) -> Option<&'static str> {
    let name = field
        .to_string()
        .trim_matches(['\'', '"'])
        .to_ascii_uppercase();
    match dialect {
        SqlDialect::Postgres | SqlDialect::Other("generic") => Some("DECIMAL"),
        SqlDialect::MySql => Some("BIGINT"),
        SqlDialect::Other("duckdb") => Some(date_part_type(&name)),
        SqlDialect::ClickHouse => clickhouse_extract_type(&name),
        _ => None,
    }
}

fn clickhouse_extract_type(field: &str) -> Option<&'static str> {
    Some(match field {
        "QUARTER" | "MONTH" | "MONTHS" | "WEEK" | "WEEKS" | "DAY" | "DAYS" | "DOW" | "ISODOW"
        | "HOUR" | "HOURS" | "MINUTE" | "MINUTES" | "SECOND" | "SECONDS" => "UTINYINT",
        "YEAR" | "YEARS" | "DOY" | "ISOYEAR" | "DECADE" | "MILLISECOND" | "MILLISECONDS" => {
            "USMALLINT"
        }
        "MICROSECOND" | "MICROSECONDS" | "NANOSECOND" | "NANOSECONDS" | "EPOCH" => "UINT",
        "TIMEZONE_HOUR" => "INT",
        "CENTURY" | "MILLENNIUM" | "TIMEZONE_MINUTE" => "BIGINT",
        _ => return None,
    })
}

/// The type DuckDB extracts `part` as: "Except for julian and epoch, which return DOUBLEs,
/// all parts are extracted as integers."
pub(super) fn date_part_type(part: &str) -> &'static str {
    if part.eq_ignore_ascii_case("epoch") || part.eq_ignore_ascii_case("julian") {
        "DOUBLE"
    } else {
        "BIGINT"
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

/// `NULL`: no value of its own to type.
fn valueless(expr: &Expr) -> bool {
    match expr {
        Expr::Nested(inner) => valueless(inner),
        Expr::Value(value) => matches!(value.value, Value::Null),
        _ => false,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use sqlparser::ast::Function;

    use super::super::test_support::expr;
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
}
