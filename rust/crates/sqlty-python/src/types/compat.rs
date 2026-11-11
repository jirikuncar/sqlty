//! Compatibility of a resolved row type with SQL column types.

use ruff_python_ast::{Expr, Number};

use super::info::TypeInfo;
use super::resolve::{annotation_type, slice_items};
use super::stdlib::Version;
use super::ty::{builtins, column_type, is_subtype, Lit, Promotions, Ty};

/// `check_type_compatibility`: whether the field of `user_type` that `column_name` fills
/// can hold values of the inferred `column_python_type` (`int`, `datetime.datetime`).
///
/// `Any`, unions and the `int` to `float` promotion are accepted; narrower fields (`bool`
/// for an `int` column) are not, unless the field is lax. An annotation that can't be
/// resolved or decided counts as compatible.
pub fn check_type_compatibility(
    user_type: &TypeInfo,
    column_name: &str,
    column_python_type: Option<&str>,
) -> bool {
    let field = if user_type.has_field(column_name) {
        user_type
            .field_types
            .get(column_name)
            .and_then(Option::as_ref)
    } else if user_type.extra_items.is_some() {
        user_type.extra_items_type.as_ref()
    } else {
        return column_python_type.is_none();
    };
    let promotions = if user_type.lax.contains(column_name) {
        Promotions::Lax
    } else {
        Promotions::Numeric
    };
    accepts(field, column_python_type, promotions)
}

pub fn accepts(
    field: Option<&Ty>,
    column_python_type: Option<&str>,
    promotions: Promotions,
) -> bool {
    let Some(column) = column_python_type.filter(|column| !column.is_empty()) else {
        return true;
    };
    let erased = field.map(Ty::erase);
    is_subtype(column_type(column).as_ref(), erased.as_ref(), promotions) != Some(false)
}

/// `field_type`: the resolved annotation of the field that holds `column_name`, `| None`
/// kept.
pub fn field_type<'a>(user_type: &'a TypeInfo, column_name: &str) -> Option<&'a Ty> {
    if user_type.has_field(column_name) {
        user_type
            .field_types
            .get(column_name)
            .and_then(Option::as_ref)
    } else if user_type.extra_items.is_some() {
        user_type.extra_items_type.as_ref()
    } else {
        None
    }
}

/// `field_accepts`: [`check_type_compatibility`] for an annotation known only as text, such
/// as an item of `SQL[tuple[...]]`.
pub fn field_accepts(annotation: &str, column_python_type: Option<&str>, version: Version) -> bool {
    accepts(
        annotation_type(annotation, version).as_ref(),
        column_python_type,
        Promotions::Numeric,
    )
}

/// `column_literal_values`: the values of an inferred `Literal` column type, `None` when it
/// is not one. `None` members (a nullable column) are left out.
pub fn column_literal_values(python_type: &str) -> Option<Vec<Lit>> {
    let parsed = crate::parse::expression(python_type)?;
    let mut members: Vec<&Expr> = vec![&parsed.syntax().body];
    while members
        .iter()
        .any(|m| matches!(m, Expr::BinOp(op) if op.op == ruff_python_ast::Operator::BitOr))
    {
        members = members
            .into_iter()
            .flat_map(|member| match member {
                Expr::BinOp(op) => vec![op.left.as_ref(), op.right.as_ref()],
                other => vec![other],
            })
            .collect();
    }
    let mut values = Vec::new();
    for member in members {
        let sub = match member {
            Expr::NoneLiteral(_) => continue,
            Expr::Subscript(sub) => sub,
            _ => return None,
        };
        if !(sub.value.is_attribute_expr()
            && crate::unparse::unparse(&sub.value, python_type) == "typing.Literal")
        {
            return None;
        }
        for item in slice_items(&sub.slice) {
            values.push(match item {
                Expr::StringLiteral(text) => Lit::Str(crate::parse::text(text, python_type)),
                Expr::BooleanLiteral(value) => Lit::Bool(value.value),
                Expr::NumberLiteral(number) if matches!(number.value, Number::Int(_)) => {
                    Lit::Int(crate::unparse::unparse(item, python_type))
                }
                _ => return None,
            });
        }
    }
    (!values.is_empty()).then_some(values)
}

/// A literal value as a member of a Python `set`, where `True` is `1`.
pub fn set_member(value: &Lit) -> Lit {
    match value {
        Lit::Bool(value) => Lit::Int(u8::from(*value).to_string()),
        other => other.clone(),
    }
}

/// `declared_literal_values`: the values a resolved annotation admits for a column that
/// takes `inferred`, as a `set` holds them: equal values once (`True` is `1`).
///
/// `None` when the annotation also takes values by class (`str` for string values, `int`
/// for integers, `Any`), or when it can't be told (an enum or other user class): only an
/// annotation made of `Literal` members and `None` is a set.
pub fn declared_literal_values(tp: &Ty, inferred: &[Lit]) -> Option<Vec<Lit>> {
    let b = builtins();
    let covering = if matches!(inferred.first(), Some(Lit::Str(_))) {
        vec![&b.str]
    } else {
        vec![&b.int, &b.float, &b.complex]
    };
    let mut admitted: Vec<Lit> = Vec::new();
    for member in tp.members() {
        let mut member = member;
        while let Ty::NewType(new) = member {
            member = &new.supertype;
        }
        match member {
            Ty::Literal(values) => {
                for value in values {
                    if !admitted.iter().any(|v| set_member(v) == set_member(value)) {
                        admitted.push(value.clone());
                    }
                }
            }
            Ty::Class(_) if member.is_none() => {}
            Ty::Class(class)
                if member.is_any()
                    || member.is(&b.object)
                    || covering.iter().any(|c| member.is(c))
                    || class.module != "builtins" =>
            {
                return None;
            }
            Ty::Class(_) => {}
            _ => return None,
        }
    }
    Some(admitted)
}

/// `parse_tuple_annotation`: the items of `tuple[int, str]` as `(_0, int)`, `(_1, str)`.
pub fn parse_tuple_annotation(annotation: &str) -> Option<Vec<(String, String)>> {
    let inner = annotation.strip_prefix("tuple[")?.strip_suffix(']')?;
    Some(
        split_type_args(inner)
            .into_iter()
            .enumerate()
            .map(|(i, item)| (format!("_{i}"), super::ty::py_strip(&item).to_string()))
            .collect(),
    )
}

/// `_split_type_args`: comma-separated type arguments, nested brackets kept whole.
pub fn split_type_args(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(super::ty::py_strip(&current).to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(super::ty::py_strip(&current).to_string());
    }
    parts
}
