//! The JSON a query builds against the annotation of the row field that receives it.
//!
//! The engine reports the shape of a `jsonb_build_object()`, `jsonb_agg()` or `to_jsonb()`
//! column (see `sqlty_sql::json`); drivers decode the value into dicts, lists and scalars.
//! [`json_problems`] compares that shape with the annotation, following nested `TypedDict`s,
//! `list[...]` and `dict[str, ...]`, and lists what cannot hold it.
//!
//! Only what is decidable is reported: `Any`, `object`, classes sqlty cannot see into
//! (imports, Pydantic models) and annotations it cannot read accept anything.

use std::collections::HashMap;
use std::sync::Arc;

use ruff_python_ast::{Expr, Operator};
use sqlty_sql::json::{JsonField, JsonShape, JsonValue};

use crate::types::info::{Annotation, Dict, Kind, TypeInfo};
use crate::types::resolve::slice_items;

const LISTS: [&str; 6] = [
    "list",
    "List",
    "Sequence",
    "MutableSequence",
    "Collection",
    "Iterable",
];
const MAPPINGS: [&str; 4] = ["dict", "Dict", "Mapping", "MutableMapping"];
/// Classes that can never hold a JSON value of another kind.
const BUILTINS: [&str; 9] = [
    "bool",
    "int",
    "float",
    "str",
    "bytes",
    "tuple",
    "Tuple",
    "set",
    "frozenset",
];
const OPEN: [&str; 2] = ["Any", "object"];
const MAX_PROBLEMS: usize = 3;
const MAX_DEPTH: usize = 32;

struct Context<'a> {
    defs: &'a Dict<Arc<TypeInfo>>,
    aliases: &'a HashMap<String, String>,
}

/// An alternative of an annotation: `None`, or an expression.
enum Member {
    None,
    Node(Annotation),
}

/// What keeps `annotation` from holding the JSON of `shape`, as sentences; empty if it fits.
///
/// `defs` are the user types of the file and `aliases` its typing names. Each sentence
/// starts with the path of the value below `name`: `items[].price is int but ...`.
pub fn json_problems(
    shape: &JsonShape,
    annotation: &Annotation,
    defs: &Dict<Arc<TypeInfo>>,
    aliases: &HashMap<String, String>,
    name: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let value = JsonValue::new(shape.clone(), false);
    check(
        &value,
        annotation,
        name,
        &Context { defs, aliases },
        &mut out,
        0,
    );
    out
}

/// The first few problems on one line, the rest counted.
pub fn summarize(problems: &[String]) -> String {
    let shown = problems[..problems.len().min(MAX_PROBLEMS)].join("; ");
    if problems.len() > MAX_PROBLEMS {
        format!("{shown}; and {} more", problems.len() - MAX_PROBLEMS)
    } else {
        shown
    }
}

/// The name a (possibly subscripted) annotation is, `typing` aliases resolved.
fn head<'e>(node: &'e Expr, ctx: &'e Context<'_>) -> Option<&'e str> {
    let node = match node {
        Expr::Subscript(sub) => sub.value.as_ref(),
        other => other,
    };
    match node {
        Expr::Name(name) => {
            let id = name.id.as_str();
            Some(ctx.aliases.get(id).map_or(id, String::as_str))
        }
        Expr::Attribute(attr) => Some(attr.attr.as_str()),
        _ => None,
    }
}

/// The alternatives of a union, `Optional` and `Annotated` looked through.
fn members(node: &Annotation, ctx: &Context<'_>, depth: usize) -> Vec<Member> {
    let part = |expr: &Expr| Annotation {
        expr: expr.clone(),
        source: node.source.clone(),
    };
    if depth > MAX_DEPTH {
        return vec![Member::Node(node.clone())];
    }
    match &node.expr {
        Expr::NoneLiteral(_) => return vec![Member::None],
        Expr::StringLiteral(text) => {
            let value = crate::parse::text(text, &node.source);
            return match crate::parse::expression(&value) {
                Some(parsed) => members(
                    &Annotation {
                        expr: *parsed.into_syntax().body,
                        source: Arc::from(value),
                    },
                    ctx,
                    depth + 1,
                ),
                None => vec![Member::Node(node.clone())],
            };
        }
        Expr::BinOp(op) if op.op == Operator::BitOr => {
            let mut found = members(&part(&op.left), ctx, depth + 1);
            found.extend(members(&part(&op.right), ctx, depth + 1));
            return found;
        }
        Expr::Subscript(sub) => {
            let args = slice_items(&sub.slice);
            match (head(&node.expr, ctx), args.first()) {
                (Some("Optional"), Some(first)) => {
                    let mut found = members(&part(first), ctx, depth + 1);
                    found.push(Member::None);
                    return found;
                }
                (Some("Union"), _) => {
                    return args
                        .iter()
                        .flat_map(|arg| members(&part(arg), ctx, depth + 1))
                        .collect();
                }
                (Some("Annotated"), Some(first)) => return members(&part(first), ctx, depth + 1),
                _ => {}
            }
        }
        _ => {}
    }
    vec![Member::Node(node.clone())]
}

fn admits_none(members: &[Member], ctx: &Context<'_>) -> bool {
    members.iter().any(|member| match member {
        Member::None => true,
        Member::Node(node) => match head(&node.expr, ctx) {
            Some(name) if OPEN.contains(&name) => true,
            Some("Literal") => matches!(&node.expr, Expr::Subscript(sub)
                if slice_items(&sub.slice).iter().any(Expr::is_none_literal_expr)),
            _ => false,
        },
    })
}

fn describe(shape: &JsonShape) -> &'static str {
    match shape {
        JsonShape::Any => "any",
        JsonShape::Never => "never",
        JsonShape::Null => "null",
        JsonShape::Boolean => "a boolean",
        JsonShape::Integer => "an integer",
        JsonShape::Number => "a number",
        JsonShape::String => "a string",
        JsonShape::Array(_) => "an array",
        JsonShape::Object(_) => "an object",
        JsonShape::Map(_) => "a map",
    }
}

fn at(path: &str, text: &str) -> String {
    if path.is_empty() {
        text.to_string()
    } else {
        format!("{path} {text}")
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

fn check(
    value: &JsonValue,
    node: &Annotation,
    path: &str,
    ctx: &Context<'_>,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let members = members(node, ctx, 0);
    if value.nullable && !admits_none(&members, ctx) {
        out.push(at(
            path,
            &format!("can be null but is declared {}", node.unparse()),
        ));
    }
    let shape = &value.shape;
    if matches!(shape, JsonShape::Any | JsonShape::Never | JsonShape::Null) {
        return;
    }
    let candidates: Vec<&Annotation> = members
        .iter()
        .filter_map(|member| match member {
            Member::Node(node) => Some(node),
            Member::None => None,
        })
        .collect();
    if candidates.is_empty() {
        out.push(at(
            path,
            &format!("is None but the query builds {}", describe(shape)),
        ));
        return;
    }
    let mut first: Option<Vec<String>> = None;
    for member in candidates {
        let mut found = Vec::new();
        check_member(shape, member, path, ctx, &mut found, depth);
        if found.is_empty() {
            return;
        }
        first.get_or_insert(found);
    }
    out.extend(first.unwrap_or_default());
}

fn mismatch(shape: &JsonShape, member: &Annotation, path: &str, out: &mut Vec<String>) {
    out.push(at(
        path,
        &format!(
            "is {} but the query builds {}",
            member.unparse(),
            describe(shape)
        ),
    ));
}

/// The annotation heads a JSON scalar decodes into (bool is an int, an int a float; a number
/// is an int when integral, else a float, and `Decimal` loads it exactly).
fn scalar_heads(shape: &JsonShape) -> &'static [&'static str] {
    match shape {
        JsonShape::Boolean => &["bool", "int", "float"],
        JsonShape::Integer => &["int", "float"],
        JsonShape::Number => &["float", "int", "Decimal"],
        JsonShape::String => &["str"],
        _ => &[],
    }
}

fn check_member(
    shape: &JsonShape,
    member: &Annotation,
    path: &str,
    ctx: &Context<'_>,
    out: &mut Vec<String>,
    depth: usize,
) {
    let Some(head) = head(&member.expr, ctx).filter(|head| !OPEN.contains(head)) else {
        return;
    };
    let row_type = member
        .expr
        .is_name_expr()
        .then(|| ctx.defs.get(head))
        .flatten()
        .filter(|info| matches!(info.kind, Kind::TypedDict | Kind::BaseModel));
    let (builtin, list, mapping) = (
        BUILTINS.contains(&head),
        LISTS.contains(&head),
        MAPPINGS.contains(&head),
    );
    let part = |expr: &Expr| Annotation {
        expr: expr.clone(),
        source: member.source.clone(),
    };
    match shape {
        JsonShape::Array(items) => {
            if list {
                if let Expr::Subscript(sub) = &member.expr {
                    let inner = format!("{path}[]");
                    check(items, &part(&sub.slice), &inner, ctx, out, depth + 1);
                }
            } else if builtin || mapping || row_type.is_some() {
                mismatch(shape, member, path, out);
            }
        }
        JsonShape::Map(values) => {
            if mapping {
                if let Some(parts) = mapping_parts(member, path, ctx, out) {
                    let inner = format!("{path}[key]");
                    check(values, &parts, &inner, ctx, out, depth + 1);
                }
            } else if builtin || list || row_type.is_some() {
                mismatch(shape, member, path, out);
            }
        }
        JsonShape::Object(fields) => {
            if mapping {
                if let Some(parts) = mapping_parts(member, path, ctx, out) {
                    for field in fields {
                        let inner = join(path, &field.name);
                        check(&field.value, &parts, &inner, ctx, out, depth + 1);
                    }
                }
            } else if let Some(info) = row_type {
                check_row_type(fields, info, path, ctx, out, depth);
            } else if builtin || list {
                mismatch(shape, member, path, out);
            }
        }
        scalar => {
            if (builtin || list || mapping || row_type.is_some())
                && !scalar_heads(scalar).contains(&head)
            {
                mismatch(shape, member, path, out);
            }
        }
    }
}

/// The value annotation of `dict[K, V]`, reporting a key type that is not `str`.
fn mapping_parts(
    member: &Annotation,
    path: &str,
    ctx: &Context<'_>,
    out: &mut Vec<String>,
) -> Option<Annotation> {
    let Expr::Subscript(sub) = &member.expr else {
        return None;
    };
    let Expr::Tuple(tuple) = sub.slice.as_ref() else {
        return None;
    };
    let (key, item) = (tuple.elts.first()?, tuple.elts.last()?);
    if head(key, ctx).is_some_and(|head| BUILTINS.contains(&head) && head != "str") {
        let key = crate::unparse::unparse(key, &member.source);
        out.push(at(
            path,
            &format!("has keys of {key} but JSON object keys are strings"),
        ));
    }
    Some(Annotation {
        expr: item.clone(),
        source: member.source.clone(),
    })
}

fn check_row_type(
    fields: &[JsonField],
    info: &TypeInfo,
    path: &str,
    ctx: &Context<'_>,
    out: &mut Vec<String>,
    depth: usize,
) {
    let inner = Context {
        defs: ctx.defs,
        aliases: if info.aliases.is_empty() {
            ctx.aliases
        } else {
            &info.aliases
        },
    };
    let built: Dict<&JsonField> = fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect();
    // A Pydantic model reads keys by alias; its field types are not checked here.
    let model = info.kind == Kind::BaseModel;
    let admits_extra = info
        .extra_items
        .as_deref()
        .is_some_and(|extra| !extra.is_empty())
        || info.allows_extra
        || !info.opaque.is_empty();
    for (name, field) in built.iter() {
        let location = join(path, name);
        let Some(key) = info.field_for(name) else {
            if !admits_extra {
                let object = if path.is_empty() { "the object" } else { path };
                out.push(at(
                    object,
                    &format!("has key '{name}' that {} does not declare", info.name),
                ));
            }
            continue;
        };
        if field.optional && info.required.contains(key) {
            out.push(at(
                &location,
                &format!("is required by {} but the query may omit it", info.name),
            ));
        }
        if !model {
            if let Some(annotation) = info.annotations.get(key) {
                check(&field.value, annotation, &location, &inner, out, depth + 1);
            }
        }
    }
    for (key, _) in &info.fields {
        let names = info.column_names(key);
        if info.required.contains(key)
            && !info.opaque.contains(key)
            && !names.iter().any(|name| built.contains(name))
        {
            if let Some(first) = names.first() {
                out.push(at(
                    &join(path, first),
                    &format!(
                        "is required by {} but the query does not build it",
                        info.name
                    ),
                ));
            }
        }
    }
}
