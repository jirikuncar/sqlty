//! Literal value sets: the values a column or expression can take, as `typing.Literal[...]`.
//!
//! A set travels inside the Python type string (`typing.Literal['a', 'b']`), next to the
//! SQL type, so views, CTEs and subqueries carry it without extra state. This module
//! renders and parses those strings and reads the sets `CHECK` constraints give.
//!
//! A set is always a superset of what the expression can take: a `CHECK` that is not a
//! plain list of constants gives no set, and a set that would exceed [`MAX_VALUES`] is
//! dropped in favour of the plain `str` or `int`.

use rustc_hash::FxHashSet;
use sqlparser::ast::InTarget;
use sqlparser::ast::{BinaryOperator, CastKind, Expr, Ident, Value as SqlValue, ValueWithSpan};

/// The most values a derived set (a `CHECK` list, a `CASE`, a `UNION`) may hold before it
/// widens to `str` or `int`. An enum keeps all its labels.
pub(crate) const MAX_VALUES: usize = 32;

const PREFIX: &str = "typing.Literal[";

/// One member of a set; a set holds only one kind.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Value {
    Str(String),
    Int(i64),
}

/// `values` as a `typing.Literal`; `None` when there are none or they mix strings and
/// integers.
pub(crate) fn render(values: &[Value]) -> Option<String> {
    let first = values.first()?;
    if values
        .iter()
        .any(|v| std::mem::discriminant(v) != std::mem::discriminant(first))
    {
        return None;
    }
    let items: Vec<String> = values
        .iter()
        .map(|value| match value {
            Value::Str(text) => quote(text),
            Value::Int(number) => number.to_string(),
        })
        .collect();
    Some(format!("{PREFIX}{}]", items.join(", ")))
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32 & 0xff)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// The values of a `typing.Literal[...]` type string; `None` for any other type.
pub(crate) fn parse(python_type: &str) -> Option<Vec<Value>> {
    let inner = python_type.strip_prefix(PREFIX)?.strip_suffix(']')?;
    let mut values = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        match chars.peek()? {
            '\'' => {
                chars.next();
                let mut text = String::new();
                loop {
                    match chars.next()? {
                        '\'' => break,
                        '\\' => match chars.next()? {
                            'n' => text.push('\n'),
                            'r' => text.push('\r'),
                            't' => text.push('\t'),
                            'x' => {
                                let hex: String = chars.by_ref().take(2).collect();
                                text.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
                            }
                            other => text.push(other),
                        },
                        c => text.push(c),
                    }
                }
                values.push(Value::Str(text));
            }
            _ => {
                let mut number = String::new();
                while let Some(c) = chars.peek().filter(|c| **c != ',') {
                    number.push(*c);
                    chars.next();
                }
                values.push(Value::Int(number.trim().parse().ok()?));
            }
        }
        match chars.next() {
            None => return Some(values),
            Some(',') => {
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
            Some(_) => return None,
        }
    }
}

/// The Python type a `Literal` of `python_type` is a subtype of: `str` or `int`.
pub(crate) fn base(python_type: &str) -> Option<&'static str> {
    if !python_type.starts_with(PREFIX) {
        return None;
    }
    Some(if python_type[PREFIX.len()..].starts_with('\'') {
        "str"
    } else {
        "int"
    })
}

/// The members of both sets, in first-seen order.
pub(crate) fn union(mut left: Vec<Value>, right: Vec<Value>) -> Vec<Value> {
    let mut seen: FxHashSet<Value> = left.iter().cloned().collect();
    for value in right {
        if seen.insert(value.clone()) {
            left.push(value);
        }
    }
    left
}

pub(crate) fn intersect(left: Vec<Value>, right: &[Value]) -> Vec<Value> {
    let right: FxHashSet<&Value> = right.iter().collect();
    left.into_iter().filter(|v| right.contains(v)).collect()
}

/// `left` without the values of `right`.
pub(crate) fn difference(left: &[Value], right: &[Value]) -> Vec<Value> {
    let right: FxHashSet<&Value> = right.iter().collect();
    left.iter()
        .filter(|v| !right.contains(v))
        .cloned()
        .collect()
}

/// The values `expr` (a `CHECK` condition) limits the column `is_target` recognizes to.
///
/// Reads `col IN ('a', 'b')`, `col = 'a'`, `col = ANY (ARRAY['a', 'b'])` (also with the
/// casts `pg_dump` adds), `OR` of those (a `col IS NULL` arm adds nothing: NULL passes
/// every `CHECK`) and `AND` (the conditions it does not understand are ignored, which
/// keeps the set a superset). Anything else gives `None`.
pub(crate) fn check_values(expr: &Expr, is_target: &dyn Fn(&Ident) -> bool) -> Option<Vec<Value>> {
    match expr {
        Expr::Nested(inner) => check_values(inner, is_target),
        Expr::Cast { expr: inner, .. } => check_values(inner, is_target),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => match (
            check_values(left, is_target),
            check_values(right, is_target),
        ) {
            (Some(a), Some(b)) => Some(intersect(a, &b)),
            (a, b) => a.or(b),
        },
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let arm = |arm: &Expr| {
                if is_null_test(arm, is_target) {
                    Some(Vec::new())
                } else {
                    check_values(arm, is_target)
                }
            };
            Some(union(arm(left)?, arm(right)?))
        }
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Eq,
            right,
        } => {
            if is_column(left, is_target) {
                constant(right).map(|v| vec![v])
            } else if is_column(right, is_target) {
                constant(left).map(|v| vec![v])
            } else {
                None
            }
        }
        Expr::In {
            expr: column,
            target: InTarget::List(list),
            negated: false,
            ..
        } if is_column(column, is_target) => list
            .iter()
            .map(constant)
            .collect::<Option<Vec<_>>>()
            .map(|all| union(Vec::new(), all)),
        Expr::AnyOp {
            left,
            compare_op: BinaryOperator::Eq,
            right,
            ..
        } if is_column(left, is_target) => array_constants(right),
        _ => None,
    }
}

fn array_constants(expr: &Expr) -> Option<Vec<Value>> {
    match expr {
        Expr::Nested(inner) | Expr::Cast { expr: inner, .. } => array_constants(inner),
        Expr::Array(array) => array
            .elem
            .iter()
            .map(constant)
            .collect::<Option<Vec<_>>>()
            .map(|all| union(Vec::new(), all)),
        _ => None,
    }
}

fn is_null_test(expr: &Expr, is_target: &dyn Fn(&Ident) -> bool) -> bool {
    match expr {
        Expr::Nested(inner) => is_null_test(inner, is_target),
        Expr::IsNull(column) => is_column(column, is_target),
        _ => false,
    }
}

fn is_column(expr: &Expr, is_target: &dyn Fn(&Ident) -> bool) -> bool {
    match expr {
        Expr::Identifier(ident) => is_target(ident),
        Expr::Nested(inner) | Expr::Cast { expr: inner, .. } => is_column(inner, is_target),
        Expr::CompoundIdentifier(parts) => parts.last().is_some_and(is_target),
        _ => false,
    }
}

/// A string or integer constant, possibly cast (`'a'::text`).
pub(crate) fn constant(expr: &Expr) -> Option<Value> {
    match expr {
        Expr::Nested(inner) => constant(inner),
        Expr::Cast {
            kind: CastKind::DoubleColon | CastKind::Cast,
            expr: inner,
            ..
        } => constant(inner),
        Expr::Value(ValueWithSpan { value, .. }) => match value {
            SqlValue::SingleQuotedString(text) => Some(Value::Str(text.clone())),
            SqlValue::Number(number, _) => number.parse().ok().map(Value::Int),
            _ => None,
        },
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr: inner,
        } => match constant(inner)? {
            Value::Int(number) => number.checked_neg().map(Value::Int),
            Value::Str(_) => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // `MAX_VALUES` keeps a set a public call reads to 32 values, so no public call reaches
    // the set functions with a large one.
    #[test]
    fn set_operations_on_large_sets_take_linear_time() {
        let many: Vec<Value> = (0..100_000).map(Value::Int).collect();
        assert_eq!(union(many.clone(), many.clone()), many);
        assert_eq!(intersect(many.clone(), &many), many);
        assert!(difference(&many, &many).is_empty());
        assert_eq!(difference(&many, &many[1..]), [Value::Int(0)]);
    }
}
