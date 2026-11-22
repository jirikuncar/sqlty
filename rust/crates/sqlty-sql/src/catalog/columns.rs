//! Columns as the loader holds them before resolution: declared types, `CHECK`-derived
//! allowed values and nullability.
//!
//! Nothing here looks types up in the catalog; [`super::resolve`] does that.

use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions, ColumnDef, ColumnOption, DataType, Expr, FunctionArg, FunctionArgExpr,
    FunctionArguments, GeneratedAs, Ident, IndexColumn, ObjectName,
};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::analyzer::SqlDialect;
use crate::values::{self, Value};

use super::loader::RawUnique;
use super::names::fold;
use super::names::parts;

#[derive(Clone, PartialEq)]
pub(super) struct RawColumn {
    pub(super) name: String,
    pub(super) ident: String,
    pub(super) ty: DataType,
    pub(super) not_null: bool,
    /// An INSERT that leaves it out still gives it a value: a `DEFAULT`, `serial`,
    /// identity, generated or auto-increment column, or SQLite's rowid.
    pub(super) has_default: bool,
    /// The `DEFAULT` an INSERT that leaves the column out writes.
    pub(super) default: Option<Box<Expr>>,
    /// The expression a generated column is computed from.
    pub(super) generated: Option<Box<Expr>>,
    /// Part of the primary key, which keeps it `NOT NULL` whatever a MySQL `MODIFY` says.
    pub(super) primary_key: bool,
    /// Where it stands in the primary key, which a `REFERENCES` without columns follows.
    pub(super) key_position: usize,
    /// The values a `CHECK` constraint limits the column to, when it lists them.
    pub(super) values: Option<Vec<Value>>,
    /// Compared with a collation that may equate different strings (`NOCASE`, `_ci`):
    /// the stored values are not the ones a `CHECK` or a domain lists.
    pub(super) collated: bool,
    /// MySQL's `INVISIBLE`: left out of `SELECT *`.
    pub(super) invisible: bool,
}

impl RawColumn {
    pub(super) fn checks(&self) -> Checks<'_> {
        Checks {
            values: self.values.as_deref(),
            collated: self.collated,
        }
    }
}

/// What constraints say about a column's values.
#[derive(Clone, Copy, Default)]
pub(super) struct Checks<'a> {
    pub(super) values: Option<&'a [Value]>,
    pub(super) collated: bool,
}

impl Checks<'_> {
    /// The Python type of a column: its `CHECK` values narrow a `Literal`, a collation
    /// removes the ones a domain gave (an enum's labels are exact either way).
    pub(super) fn apply(
        self,
        python: Option<String>,
        sql_type: &str,
        is_enum: bool,
    ) -> Option<String> {
        if self.collated && !is_enum {
            return None;
        }
        narrowed(python, sql_type, self.values)
    }
}

/// Whether a collation name may equate different strings; `BINARY`, `C` and `POSIX` do not.
pub(super) fn loose_collation(name: &ObjectName) -> bool {
    let last = name.0.last().map(|part| {
        part.to_string()
            .trim_matches(['"', '`', '\''])
            .to_lowercase()
    });
    !matches!(
        last.as_deref(),
        Some("binary" | "c" | "posix" | "ucs_basic")
    )
}

pub(super) fn raw_column(column: &ColumnDef, dialect: SqlDialect) -> RawColumn {
    // `serial` is an integer with a sequence default, which SQLite only names.
    let serial = match &column.data_type {
        DataType::Custom(name, _) => match parts(name, dialect).last().map(String::as_str) {
            Some("serial" | "serial4") => Some(DataType::Integer(None)),
            Some("bigserial" | "serial8") => Some(DataType::BigInt(None)),
            Some("smallserial" | "serial2") => Some(DataType::SmallInt(None)),
            _ => None,
        },
        _ => None,
    };
    let ty = match (&serial, dialect) {
        (_, SqlDialect::Sqlite) | (None, _) => column.data_type.clone(),
        (Some(_), SqlDialect::MySql) => DataType::BigIntUnsigned(None),
        (Some(integer), _) => integer.clone(),
    };
    let serial = serial.is_some();
    let not_null = serial
        || column.options.iter().any(|option| {
            matches!(
                option.option,
                ColumnOption::NotNull | ColumnOption::PrimaryKey(_)
            ) || matches!(
                &option.option,
                ColumnOption::Generated {
                    generated_as: GeneratedAs::Always | GeneratedAs::ByDefault,
                    generation_expr: None,
                    ..
                }
            )
        });
    let primary_key = column
        .options
        .iter()
        .any(|option| matches!(option.option, ColumnOption::PrimaryKey(_)));
    let has_default = serial
        || (dialect == SqlDialect::Sqlite && primary_key && is_rowid_type(&column.data_type))
        || column.options.iter().any(|option| match &option.option {
            ColumnOption::Default(_)
            | ColumnOption::Generated { .. }
            | ColumnOption::Identity(_)
            | ColumnOption::Materialized(_)
            | ColumnOption::Alias(_) => true,
            ColumnOption::DialectSpecific(tokens) => tokens.iter().any(|token| {
                let word = token.to_string();
                word.eq_ignore_ascii_case("AUTO_INCREMENT")
                    || word.eq_ignore_ascii_case("AUTOINCREMENT")
            }),
            _ => false,
        });
    let name = fold(&column.name, dialect);
    // MySQL before 8.0.16 parses `CHECK` and ignores it.
    let collated = column.options.iter().any(
        |option| matches!(&option.option, ColumnOption::Collation(name) if loose_collation(name)),
    );
    let values = column
        .options
        .iter()
        .filter(|_| !collated)
        .filter_map(|option| match &option.option {
            ColumnOption::Check(check) if dialect != SqlDialect::MySql => {
                values::check_values(&check.expr, &|ident| fold(ident, dialect) == name)
            }
            _ => None,
        })
        .reduce(|a, b| values::intersect(a, &b));
    RawColumn {
        name,
        ident: match column.name.quote_style {
            Some(_) => column.name.to_string(),
            None if dialect == SqlDialect::Postgres => column.name.value.to_lowercase(),
            None => column.name.value.clone(),
        },
        ty,
        not_null,
        has_default,
        default: column
            .options
            .iter()
            .find_map(|option| match &option.option {
                ColumnOption::Default(expr) => Some(Box::new(expr.clone())),
                _ => None,
            }),
        generated: column
            .options
            .iter()
            .find_map(|option| match &option.option {
                ColumnOption::Generated {
                    generation_expr: Some(expr),
                    ..
                } => Some(Box::new(expr.clone())),
                _ => None,
            }),
        primary_key,
        key_position: 0,
        values,
        collated,
        invisible: column
            .options
            .iter()
            .any(|option| matches!(option.option, ColumnOption::Invisible)),
    }
}

/// Narrow `existing` by the values of another `CHECK`.
pub(super) fn add_values(existing: &mut Option<Vec<Value>>, found: Option<Vec<Value>>) {
    *existing = match (existing.take(), found) {
        (Some(a), Some(b)) => Some(values::intersect(a, &b)),
        (a, b) => a.or(b),
    };
}

/// Apply a table-level `CHECK` to the column it limits.
pub(super) fn apply_check(columns: &mut [RawColumn], expr: &Expr, dialect: SqlDialect) {
    for (at, found) in checked(columns, expr, dialect) {
        add_values(&mut columns[at].values, Some(found));
    }
}

/// Whether [`apply_check`] would change a column.
pub(super) fn check_narrows(columns: &[RawColumn], expr: &Expr, dialect: SqlDialect) -> bool {
    checked(columns, expr, dialect)
        .into_iter()
        .any(|(at, found)| {
            let mut values = columns[at].values.clone();
            add_values(&mut values, Some(found));
            values != columns[at].values
        })
}

/// The columns a `CHECK` limits, by position, and the values it limits each to. Only the
/// columns `expr` names are read, so a check per column of a wide table stays cheap.
fn checked(columns: &[RawColumn], expr: &Expr, dialect: SqlDialect) -> Vec<(usize, Vec<Value>)> {
    if dialect == SqlDialect::MySql {
        return Vec::new();
    }
    let mut named = FxHashSet::default();
    let _ = visit_expressions(expr, |expr| {
        match expr {
            Expr::Identifier(ident) => {
                named.insert(fold(ident, dialect));
            }
            Expr::CompoundIdentifier(parts) => named.extend(parts.last().map(|p| fold(p, dialect))),
            _ => {}
        }
        ControlFlow::<()>::Continue(())
    });
    let limits = |(at, column): (usize, &RawColumn)| {
        if !named.contains(&column.name) {
            return None;
        }
        let found = values::check_values(expr, &|ident| fold(ident, dialect) == column.name)?;
        Some((at, found))
    };
    columns.iter().enumerate().filter_map(limits).collect()
}

/// The groups of columns a `CHECK` keeps at least one of non-null, conjunct by conjunct:
/// `num_nonnulls(a, b) = 1` (or `>= 1`, `> 0`), `num_nulls(a, b) = 0` (each of them on its
/// own), `a IS NOT NULL OR b IS NOT NULL`. MySQL before 8.0.16 ignores `CHECK`.
pub(super) fn nonnull_groups(expr: &Expr, dialect: SqlDialect) -> Vec<Vec<String>> {
    use sqlparser::ast::BinaryOperator::{And, Eq, Gt, GtEq, Lt, LtEq, Or};

    if dialect == SqlDialect::MySql {
        return Vec::new();
    }
    let column = |expr: &Expr| match expr {
        Expr::Identifier(ident) => Some(fold(ident, dialect)),
        _ => None,
    };
    let count = |expr: &Expr| -> Option<(String, Vec<String>)> {
        let Expr::Function(func) = expr else {
            return None;
        };
        let name = func.name.0.last()?.as_ident()?.value.to_ascii_lowercase();
        let FunctionArguments::List(list) = &func.args else {
            return None;
        };
        let columns = list
            .args
            .iter()
            .map(|arg| match arg {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => column(expr),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some((name, columns))
    };
    let number = |expr: &Expr| match values::constant(expr) {
        Some(Value::Int(n)) => Some(n),
        _ => None,
    };
    let mut groups = Vec::new();
    let mut stack = vec![expr];
    while let Some(expr) = stack.pop() {
        match expr {
            Expr::Nested(inner) => stack.push(inner),
            Expr::BinaryOp {
                left,
                op: And,
                right,
            } => stack.extend([&**left, &**right]),
            Expr::BinaryOp { left, op, right } if matches!(op, Eq | Gt | GtEq | Lt | LtEq) => {
                // `n < num_nonnulls(...)` reads as `num_nonnulls(...) > n`.
                let (call, op, n) = match (count(left), count(right)) {
                    (Some(call), None) => (call, op.clone(), number(right)),
                    (None, Some(call)) => (
                        call,
                        match op {
                            Gt => Lt,
                            GtEq => LtEq,
                            Lt => Gt,
                            LtEq => GtEq,
                            other => other.clone(),
                        },
                        number(left),
                    ),
                    _ => continue,
                };
                let ((name, columns), Some(n)) = (call, n) else {
                    continue;
                };
                match (name.as_str(), op, n) {
                    ("num_nonnulls", Eq | GtEq, n) if n >= 1 => groups.push(columns),
                    ("num_nonnulls", Gt, n) if n >= 0 => groups.push(columns),
                    ("num_nulls", Eq, 0) => groups.extend(columns.into_iter().map(|c| vec![c])),
                    _ => {}
                }
            }
            Expr::BinaryOp { op: Or, .. } => {
                let mut columns = Vec::new();
                let mut alternatives = vec![expr];
                while let Some(alternative) = alternatives.pop() {
                    match alternative {
                        Expr::Nested(inner) => alternatives.push(inner),
                        Expr::BinaryOp {
                            left,
                            op: Or,
                            right,
                        } => {
                            alternatives.extend([&**left, &**right]);
                        }
                        Expr::IsNotNull(inner) if column(inner).is_some() => {
                            columns.extend(column(inner));
                        }
                        _ => {
                            columns.clear();
                            break;
                        }
                    }
                }
                if !columns.is_empty() {
                    groups.push(columns);
                }
            }
            Expr::IsNotNull(inner) => groups.extend(column(inner).map(|c| vec![c])),
            _ => {}
        }
    }
    groups
}

/// The Python type of a column of `sql_type` that a `CHECK` limits to `checked`: a
/// `Literal` of at most [`values::MAX_VALUES`] strings for a text column or integers for an
/// integer column. `python` (an enum's `Literal`) is kept, and narrowed by `checked`.
pub(super) fn narrowed(
    python: Option<String>,
    sql_type: &str,
    checked: Option<&[Value]>,
) -> Option<String> {
    let Some(checked) = checked else {
        return python;
    };
    if let Some(current) = python.as_deref().and_then(values::parse) {
        let both = values::intersect(current, checked);
        return values::render(&both).or(python);
    }
    if python.is_some() || checked.len() > values::MAX_VALUES {
        return python;
    }
    let name = sql_type.split(['(', '<']).next().unwrap_or_default().trim();
    let fits = match checked.first()? {
        Value::Str(_) => matches!(name, "TEXT" | "VARCHAR"),
        Value::Int(_) => matches!(
            name,
            "SMALLINT" | "INT" | "BIGINT" | "TINYINT" | "MEDIUMINT"
        ),
    };
    if fits {
        values::render(checked)
    } else {
        None
    }
}

/// Add each of `added`, or replace the same-named one in place (inherited columns merge).
pub(super) fn merge_columns(
    columns: &mut Vec<RawColumn>,
    added: impl IntoIterator<Item = RawColumn>,
) {
    let mut at: FxHashMap<String, usize> = FxHashMap::default();
    for (i, column) in columns.iter().enumerate() {
        at.entry(column.name.clone()).or_insert(i);
    }
    for column in added {
        let Some(&i) = at.get(&column.name) else {
            at.insert(column.name.clone(), columns.len());
            columns.push(column);
            continue;
        };
        let existing = &mut columns[i];
        let not_null = existing.not_null || column.not_null;
        let collated = existing.collated || column.collated;
        let mut checked = existing.values.take();
        add_values(&mut checked, column.values.clone());
        *existing = column;
        existing.not_null = not_null;
        existing.values = checked;
        existing.collated |= collated;
    }
}

/// Whether a column of this type is SQLite's rowid when it is the primary key: only the
/// type name `INTEGER`, not `INT`.
pub(super) fn is_rowid_type(ty: &DataType) -> bool {
    matches!(ty, DataType::Integer(None))
}

pub(super) fn column_named<'a>(
    columns: &'a [RawColumn],
    expr: &Expr,
    dialect: SqlDialect,
) -> Option<&'a RawColumn> {
    let Expr::Identifier(ident) = expr else {
        return None;
    };
    let name = fold(ident, dialect);
    columns.iter().find(|c| c.name == name)
}

pub(super) fn mark_primary_key(
    columns: &mut [RawColumn],
    expr: &Expr,
    dialect: SqlDialect,
    position: usize,
) {
    for column in columns.iter_mut().filter(|c| is_named(c, expr, dialect)) {
        column.not_null = true;
        column.primary_key = true;
        column.key_position = position;
    }
}

/// A key column of a SQLite table that may hold NULL.
pub(super) fn mark_loose_primary_key(
    columns: &mut [RawColumn],
    expr: &Expr,
    dialect: SqlDialect,
    position: usize,
) {
    for column in columns.iter_mut().filter(|c| is_named(c, expr, dialect)) {
        column.primary_key = true;
        column.key_position = position;
    }
}

fn is_named(column: &RawColumn, expr: &Expr, dialect: SqlDialect) -> bool {
    matches!(expr, Expr::Identifier(i) if column.name == fold(i, dialect))
}

/// The key a `UNIQUE` constraint or index declares, when every part of it is a column.
pub(super) fn unique_key(
    name: Option<&Ident>,
    columns: &[IndexColumn],
    dialect: SqlDialect,
) -> Option<RawUnique> {
    let columns = columns
        .iter()
        .map(|column| match &column.column.expr {
            Expr::Identifier(ident) => Some(fold(ident, dialect)),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;
    Some(RawUnique {
        name: name.map(|name| fold(name, dialect)),
        columns,
    })
}
