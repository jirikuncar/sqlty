//! Literal value sets in expression types.
//!
//! A column or expression whose values are known is typed `typing.Literal[...]` (see
//! [`crate::values`]); the SQL type stays next to it. Constants are `Literal`s too, with no
//! SQL type, so `CASE`, `COALESCE`, `UNION` and `VALUES` over constants unify into the set of
//! their values. Everything that computes a new value from its inputs ([`strip`]) goes back
//! to the plain type, so a set is always a superset of the values an expression can take.

use super::*;
use crate::values::{self, Value as Lit};

/// A string or integer constant.
pub(super) fn constant(value: Lit) -> Typed {
    Typed::new(values::render(&[value]), None)
}

/// The values `typed` is limited to, when it is a `Literal`.
pub(super) fn value_set(typed: &Typed) -> Option<Vec<Lit>> {
    typed.python().and_then(values::parse)
}

/// The Python type with a `Literal` collapsed to `str` or `int`, for code that tells
/// constants by their Python type.
pub(super) fn python_kind(typed: &Typed) -> Option<&str> {
    let python = typed.python()?;
    Some(values::base(python).unwrap_or(python))
}

/// `typed` without its value set: what a computation over it gives.
pub(super) fn strip(typed: Typed, dialect: SqlDialect) -> Typed {
    let Some(kind) = typed.python().and_then(values::base) else {
        return typed;
    };
    match typed.sql {
        None => Typed::python_only(kind),
        // A user-defined enum has no plain Python type.
        Some(sql_type) => Typed {
            python: python_type(&sql_type, dialect).map(Rc::from),
            sql: Some(sql_type),
        },
    }
}

/// The type of a set operation's column (or `CASE`, `COALESCE`...) over `left` and `right`
/// when either has a value set: the plain types unify as usual, and the sets unite when both
/// have one. `None` when neither has, so the caller unifies as usual.
pub(super) fn merge(left: &Typed, right: &Typed, dialect: SqlDialect) -> Option<Option<Typed>> {
    let (known_left, known_right) = (value_set(left), value_set(right));
    if known_left.is_none() && known_right.is_none() {
        return None;
    }
    let plain = common_type(
        strip(left.clone(), dialect),
        strip(right.clone(), dialect),
        dialect,
    );
    let Some(plain) = plain else {
        return Some(None);
    };
    if let (Some(a), Some(b)) = (known_left, known_right) {
        let all = values::union(a, b);
        // An enum keeps every label it has; other sets widen past the cap.
        let enum_type = plain.python.is_none() && plain.sql.is_some();
        if enum_type || all.len() <= values::MAX_VALUES {
            if let Some(literal) = values::render(&all) {
                return Some(Some(Typed {
                    python: Some(Rc::from(literal)),
                    sql: plain.sql,
                }));
            }
        }
    }
    Some(Some(plain))
}
