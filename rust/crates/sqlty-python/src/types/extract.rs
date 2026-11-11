//! The user-defined row types of a source and their fields, all from the syntax tree.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use ruff_python_ast::{Expr, Operator, Stmt, StmtClassDef, TypeParam};
use ruff_text_size::Ranged;

use super::info::{Annotation, Dict, Kind, TypeInfo};
use super::pydantic::{is_strict_name, Known};
use super::resolve::{
    get_name, is_name, qualname, slice_items, unwrap_qualifiers, Names, Resolver, QUALIFIERS,
};
use super::stdlib::{stdlib_class, Version};
use super::symbols::{Index, ModuleTypes, Source};
use super::ty::{consistent, is_subclass, is_subtype, Promotions, Ty};
use crate::unparse::unparse;

type Aliases = HashMap<String, String>;

/// A statement of a class body that says something of its fields.
pub enum Item<'a> {
    /// `name: annotation = value`.
    Field {
        name: Cow<'a, str>,
        annotation: &'a Expr,
        value: Option<&'a Expr>,
    },
    /// `a = b = value`, by the targets that are names.
    Assign {
        targets: Vec<&'a str>,
        value: &'a Expr,
    },
    Class(&'a StmtClassDef),
}

/// A class definition, or the class a functional `TypedDict(...)` / `NamedTuple(...)`
/// builds.
pub struct ClassNode<'a> {
    pub name: String,
    pub lineno: usize,
    pub bases: Vec<&'a Expr>,
    pub keywords: Vec<(Option<&'a str>, &'a Expr)>,
    pub decorators: Vec<&'a Expr>,
    pub type_params: Vec<String>,
    pub body: Vec<Item<'a>>,
}

impl<'a> ClassNode<'a> {
    pub fn of(node: &'a StmtClassDef, source: &Source) -> Self {
        let keywords = node
            .keywords()
            .iter()
            .map(|k| {
                (
                    k.arg.as_ref().map(ruff_python_ast::Identifier::as_str),
                    &k.value,
                )
            })
            .collect();
        ClassNode {
            name: node.name.to_string(),
            lineno: source.line(class_start(node, &source.text)),
            bases: node.bases().iter().collect(),
            keywords,
            decorators: node.decorator_list.iter().map(|d| &d.expression).collect(),
            type_params: type_params(node, source),
            body: node.body.iter().filter_map(item).collect(),
        }
    }

    fn keyword(&self, name: &str) -> Option<&'a Expr> {
        self.keywords
            .iter()
            .rev()
            .find(|(arg, _)| *arg == Some(name))
            .map(|(_, value)| *value)
    }

    /// `_base_names`: the bases spelled as bare names.
    fn base_names(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.bases.iter().filter_map(|base| match base {
            Expr::Name(name) => Some(name.id.as_str()),
            _ => None,
        })
    }

    fn fields(&self) -> impl Iterator<Item = (&str, &'a Expr, Option<&'a Expr>)> + '_ {
        self.body.iter().filter_map(|item| match item {
            Item::Field {
                name,
                annotation,
                value,
            } => Some((name.as_ref(), *annotation, *value)),
            _ => None,
        })
    }
}

/// Where `ast` puts a class: at `class`, after its decorators.
fn class_start(node: &StmtClassDef, source: &str) -> ruff_text_size::TextSize {
    if node.decorator_list.is_empty() {
        return node.start();
    }
    // `class` and the name may be lines apart: `class \\\n    Row:`.
    let before = source[..node.name.start().to_usize()]
        .trim_end_matches(|c: char| c.is_whitespace() || c == '\\');
    let keyword = before.len().saturating_sub("class".len());
    ruff_text_size::TextSize::try_from(keyword).unwrap_or(node.start())
}

fn item(stmt: &Stmt) -> Option<Item<'_>> {
    match stmt {
        Stmt::AnnAssign(node) => match node.target.as_ref() {
            Expr::Name(target) => Some(Item::Field {
                name: Cow::Borrowed(target.id.as_str()),
                annotation: &node.annotation,
                value: node.value.as_deref(),
            }),
            _ => None,
        },
        Stmt::Assign(node) => Some(Item::Assign {
            targets: node
                .targets
                .iter()
                .filter_map(|target| match target {
                    Expr::Name(name) => Some(name.id.as_str()),
                    _ => None,
                })
                .collect(),
            value: &node.value,
        }),
        Stmt::ClassDef(node) => Some(Item::Class(node)),
        _ => None,
    }
}

/// `_type_params`: `T` of `class Row[T]`, or of a `Generic[T]` base.
fn type_params(node: &StmtClassDef, source: &Source) -> Vec<String> {
    if let Some(params) = node.type_params.as_deref() {
        return params
            .iter()
            .filter_map(|param| match param {
                TypeParam::TypeVar(var) => Some(var.name.to_string()),
                _ => None,
            })
            .collect();
    }
    for base in node.bases() {
        if let Expr::Subscript(sub) = base {
            if unparse(&sub.value, &source.text).ends_with("Generic") {
                return slice_items(&sub.slice)
                    .iter()
                    .filter_map(|item| match item {
                        Expr::Name(name) => Some(name.id.to_string()),
                        _ => None,
                    })
                    .collect();
            }
        }
    }
    Vec::new()
}

/// `_classify_class`: the kind of row a class is. Subclasses of the names `typeddicts`
/// holds are `TypedDict`s too, and of `models` models.
pub fn classify<'e>(
    bases: impl IntoIterator<Item = &'e Expr>,
    decorators: impl IntoIterator<Item = &'e Expr>,
    aliases: &Aliases,
    typeddicts: &dyn Fn(&str) -> bool,
    models: &dyn Fn(&str) -> bool,
) -> Option<Kind> {
    for base in bases {
        let name = get_name(base).map(|name| aliases.get(name).map_or(name, String::as_str));
        let bare = match base {
            Expr::Name(name) => Some(name.id.as_str()),
            _ => None,
        };
        if name == Some("TypedDict") || bare.is_some_and(typeddicts) {
            return Some(Kind::TypedDict);
        }
        if name == Some("NamedTuple") {
            return Some(Kind::NamedTuple);
        }
        if name == Some("BaseModel") || bare.is_some_and(models) {
            return Some(Kind::BaseModel);
        }
        if name == Some("Protocol") {
            return Some(Kind::Protocol);
        }
    }
    decorators
        .into_iter()
        .any(|decorator| get_name(decorator) == Some("dataclass"))
        .then_some(Kind::Dataclass)
}

/// `_functional_classes`: `R = TypedDict("R", {...})` and `N = NamedTuple("N", [(...)])` at
/// module level, as classes at the line of their assignment.
fn functional_classes<'a>(source: &'a Source) -> Vec<ClassNode<'a>> {
    let mut classes = Vec::new();
    for stmt in source.body() {
        let Stmt::Assign(assign) = stmt else {
            continue;
        };
        let ([Expr::Name(target)], Expr::Call(call)) =
            (assign.targets.as_slice(), assign.value.as_ref())
        else {
            continue;
        };
        let [_, spec] = &*call.arguments.args else {
            continue;
        };
        let name = get_name(&call.func).unwrap_or("");
        let kind = source.aliases.get(name).map_or(name, String::as_str);
        let pairs: Option<Vec<(&Expr, &Expr)>> = match (kind, spec) {
            ("TypedDict", Expr::Dict(dict)) => dict
                .items
                .iter()
                .map(|item| item.key.as_ref().map(|key| (key, &item.value)))
                .collect(),
            ("NamedTuple", Expr::List(list)) => pair_items(&list.elts),
            ("NamedTuple", Expr::Tuple(tuple)) => pair_items(&tuple.elts),
            _ => None,
        };
        let Some(pairs) = pairs else {
            continue;
        };
        let body: Option<Vec<Item<'a>>> = pairs
            .into_iter()
            .map(|(key, value)| match key {
                Expr::StringLiteral(text) => Some(Item::Field {
                    name: Cow::Owned(crate::parse::text(text, &source.text)),
                    annotation: value,
                    value: None,
                }),
                _ => None,
            })
            .collect();
        let Some(body) = body else {
            continue;
        };
        classes.push(ClassNode {
            name: target.id.to_string(),
            lineno: source.line(stmt.start()),
            bases: vec![&call.func],
            keywords: call
                .arguments
                .keywords
                .iter()
                .map(|k| {
                    (
                        k.arg.as_ref().map(ruff_python_ast::Identifier::as_str),
                        &k.value,
                    )
                })
                .collect(),
            decorators: Vec::new(),
            type_params: Vec::new(),
            body,
        });
    }
    classes
}

fn pair_items(items: &[Expr]) -> Option<Vec<(&Expr, &Expr)>> {
    items
        .iter()
        .map(|item| match item {
            Expr::Tuple(pair) => match pair.elts.as_slice() {
                [key, value] => Some((key, value)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn is_const_true(expr: Option<&Expr>) -> bool {
    matches!(expr, Some(Expr::BooleanLiteral(value)) if value.value)
}

fn is_const_false(expr: Option<&Expr>) -> bool {
    matches!(expr, Some(Expr::BooleanLiteral(value)) if !value.value)
}

fn is_field_call(expr: &Expr) -> bool {
    matches!(expr, Expr::Call(call) if matches!(get_name(&call.func), Some("field" | "Field")))
}

/// `_takes_column`: whether a class-body annotation is a constructor argument, which a
/// column fills.
fn takes_column(
    name: &str,
    annotation: &Expr,
    value: Option<&Expr>,
    kind: Option<Kind>,
    aliases: &Aliases,
) -> bool {
    let head = match annotation {
        Expr::Subscript(sub) => &sub.value,
        other => other,
    };
    if is_name(head, "ClassVar", aliases) {
        return false;
    }
    match kind {
        Some(Kind::BaseModel) => return !name.starts_with('_'),
        Some(Kind::Dataclass) => {}
        _ => return true,
    }
    if get_name(annotation) == Some("KW_ONLY") {
        return false;
    }
    let Some(value @ Expr::Call(call)) = value else {
        return true;
    };
    !(is_field_call(value)
        && call.arguments.keywords.iter().any(|k| {
            k.arg.as_ref().is_some_and(|arg| arg.as_str() == "init")
                && is_const_false(Some(&k.value))
        }))
}

/// `_has_default`: whether a dataclass / Pydantic field assigned `value` has a default.
fn has_default(value: Option<&Expr>) -> bool {
    let Some(value) = value else {
        return false;
    };
    match value {
        Expr::Call(call) if is_field_call(value) => {
            if call.arguments.keywords.iter().any(|k| {
                k.arg.as_ref().is_some_and(|arg| {
                    matches!(arg.as_str(), "default" | "default_factory" | "factory")
                })
            }) {
                return true;
            }
            call.arguments
                .args
                .first()
                .is_some_and(|first| !matches!(first, Expr::EllipsisLiteral(_)))
        }
        _ => true,
    }
}

/// `_field_calls`: the `Field(...)` / `field(...)` calls in the `Annotated` metadata.
fn field_calls<'e>(mut annotation: &'e Expr, aliases: &Aliases) -> Vec<&'e Expr> {
    let mut calls = Vec::new();
    while let Expr::Subscript(sub) = annotation {
        let name = get_name(&sub.value).map(|name| aliases.get(name).map_or(name, String::as_str));
        match (name, sub.slice.as_ref()) {
            (Some("Annotated"), Expr::Tuple(tuple)) => {
                let Some((first, metadata)) = tuple.elts.split_first() else {
                    break;
                };
                calls.extend(metadata.iter().filter(|m| is_field_call(m)));
                annotation = first;
            }
            (Some(name), slice) if QUALIFIERS.contains(&name) => annotation = slice,
            _ => break,
        }
    }
    calls
}

fn metadata_has_default(annotation: &Expr, aliases: &Aliases) -> bool {
    field_calls(annotation, aliases)
        .into_iter()
        .any(|call| has_default(Some(call)))
}

/// `_normalize_type_str`: a type string without its `Optional` / `None` wrappers.
pub fn normalize_type_str(text: &str) -> String {
    let mut text = text.replace(" | None", "").replace("None | ", "");
    if text.starts_with("Optional[") && text.ends_with(']') {
        text = text[9..text.len() - 1].to_string();
    }
    super::ty::py_strip(&text).to_string()
}

/// `_extract_fields_from_ast`: the annotated fields as `(name, type)` and the required names.
fn own_fields(
    node: &ClassNode<'_>,
    source: &Source,
    total: bool,
    kind: Kind,
) -> (Vec<(String, String)>, BTreeSet<String>) {
    let aliases = &source.aliases;
    let defaults_optional = matches!(kind, Kind::Dataclass | Kind::BaseModel);
    let mut fields = Vec::new();
    let mut required = BTreeSet::new();
    for (name, annotation, value) in node.fields() {
        if !takes_column(name, annotation, value, Some(kind), aliases) {
            continue;
        }
        let (bare, is_required) = unwrap_qualifiers(annotation, aliases);
        fields.push((
            name.to_string(),
            normalize_type_str(&unparse(bare, &source.text)),
        ));
        if metadata_has_default(annotation, aliases) {
            continue;
        }
        let needed = if defaults_optional {
            !has_default(value)
        } else {
            is_required.unwrap_or(total)
        };
        if needed {
            required.insert(name.to_string());
        }
    }
    (fields, required)
}

type Options<'a> = Dict<&'a Expr>;

/// `_config_options`: the Pydantic config of a class: `model_config` /
/// `__pydantic_config__ = ConfigDict(...)` / `{...}`, `class Config` or `config`.
fn config_options<'a>(
    node: &ClassNode<'a>,
    config: Option<&'a Expr>,
    source: &Source,
) -> Options<'a> {
    let mut options: Options<'a> = Dict::new();
    let read = |options: &mut Options<'a>, value: Option<&'a Expr>| match value {
        Some(Expr::Call(call)) => {
            for keyword in &call.arguments.keywords {
                if let Some(arg) = &keyword.arg {
                    options.insert(arg.as_str(), &keyword.value);
                }
            }
        }
        Some(Expr::Dict(dict)) => {
            for item in &dict.items {
                if let Some(Expr::StringLiteral(key)) = &item.key {
                    options.insert(crate::parse::text(key, &source.text), &item.value);
                }
            }
        }
        _ => {}
    };
    for (arg, value) in &node.keywords {
        if let Some(arg) = arg {
            options.insert(*arg, *value);
        }
    }
    read(&mut options, config);
    for item in &node.body {
        match item {
            Item::Assign { targets, value } if targets.iter().any(|t| is_config_name(t)) => {
                read(&mut options, Some(value));
            }
            Item::Field { name, value, .. } if is_config_name(name) => read(&mut options, *value),
            Item::Class(class) if class.name.as_str() == "Config" => {
                for stmt in &class.body {
                    if let Stmt::Assign(assign) = stmt {
                        for target in &assign.targets {
                            if let Expr::Name(name) = target {
                                options.insert(name.id.as_str(), &assign.value);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    options
}

fn is_config_name(name: &str) -> bool {
    matches!(name, "model_config" | "__pydantic_config__")
}

/// `_pydantic_config`: the config of a Pydantic model or `pydantic.dataclasses.dataclass`.
fn pydantic_config<'a>(node: &ClassNode<'a>, kind: Kind, source: &Source) -> Option<Options<'a>> {
    if kind == Kind::BaseModel {
        return Some(config_options(node, None, source));
    }
    for decorator in &node.decorators {
        let (callee, keywords) = match decorator {
            Expr::Call(call) => (call.func.as_ref(), &*call.arguments.keywords),
            other => (*other, &[][..]),
        };
        if qualname(callee, Some(&source.imports)).as_deref()
            == Some("pydantic.dataclasses.dataclass")
        {
            let config = keywords
                .iter()
                .find(|k| k.arg.as_ref().is_some_and(|arg| arg.as_str() == "config"))
                .map(|k| &k.value);
            return Some(config_options(node, config, source));
        }
    }
    None
}

/// `_strict`: `strict` set by `Field(strict=...)` or `Strict(...)`.
fn strict(call: &Expr) -> Option<bool> {
    let Expr::Call(call) = call else {
        return None;
    };
    let name = get_name(&call.func)?;
    if !matches!(name, "Field" | "Strict") {
        return None;
    }
    let keyword = call
        .arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().is_some_and(|arg| arg.as_str() == "strict"))
        .map(|k| &k.value);
    let value = if name == "Strict" {
        match (call.arguments.args.first(), keyword) {
            (Some(first), _) => first,
            (None, Some(keyword)) => keyword,
            (None, None) => return Some(true),
        }
    } else {
        keyword?
    };
    match value {
        Expr::BooleanLiteral(value) => Some(value.value),
        _ => None,
    }
}

/// `_lax_fields`: the fields of a Pydantic row that are not strict, by `config`, `Field`,
/// `Strict()` or `Strict*` types.
fn lax_fields(node: &ClassNode<'_>, config: &Options<'_>, source: &Source) -> BTreeSet<String> {
    let aliases = &source.aliases;
    let model = is_const_true(config.get("strict").copied());
    let mut lax = BTreeSet::new();
    for (name, annotation, value) in node.fields() {
        let mut pending = vec![annotation];
        let mut members = Vec::new();
        let mut metadata: Vec<&Expr> = Vec::new();
        while let Some(annotation) = pending.pop() {
            let head = match annotation {
                Expr::Subscript(sub) => {
                    get_name(&sub.value).map(|name| aliases.get(name).map_or(name, String::as_str))
                }
                _ => None,
            };
            match annotation {
                Expr::BinOp(op) if op.op == Operator::BitOr => {
                    pending.push(&op.left);
                    pending.push(&op.right);
                }
                Expr::Subscript(sub) => match (head, sub.slice.as_ref()) {
                    (Some("Annotated"), Expr::Tuple(tuple)) => {
                        let Some((first, rest)) = tuple.elts.split_first() else {
                            continue;
                        };
                        metadata.splice(0..0, rest.iter());
                        pending.push(first);
                    }
                    (Some(head), slice) if QUALIFIERS.contains(&head) || head == "Optional" => {
                        pending.push(slice);
                    }
                    (Some("Union"), Expr::Tuple(tuple)) => pending.extend(tuple.elts.iter()),
                    _ => members.push(annotation),
                },
                other => members.push(other),
            }
        }
        metadata.extend(value);
        let found = metadata.iter().filter_map(|m| strict(m)).next_back();
        let typed = members.iter().any(|member| {
            qualname(member, Some(&source.imports)).is_some_and(|name| is_strict_name(&name))
        });
        if !(typed || found.unwrap_or(model)) {
            lax.insert(name.to_string());
        }
    }
    lax
}

/// `_allows_extra`: whether a Pydantic model keeps undeclared columns: `extra="allow"`, or a
/// config not read from source.
fn allows_extra(node: &ClassNode<'_>, options: &Options<'_>, models: &HashSet<&str>) -> bool {
    if let Some(extra) = options.get("extra") {
        let value = match extra {
            Expr::StringLiteral(text) => Some(text.value.to_str()),
            Expr::Attribute(attr) => Some(attr.attr.as_str()),
            _ => None,
        };
        return !matches!(value, Some("ignore" | "forbid"));
    }
    if node.keywords.iter().any(|(arg, _)| arg.is_none()) {
        return true;
    }
    for base in &node.bases {
        let root = match base {
            Expr::Subscript(sub) => sub.value.as_ref(),
            other => *other,
        };
        let known = matches!(get_name(root), Some("BaseModel" | "Generic"))
            || matches!(root, Expr::Name(name) if models.contains(name.id.as_str()));
        if !known {
            return true;
        }
    }
    for item in &node.body {
        let config = match item {
            Item::Assign { targets, value } if targets.iter().any(|t| is_config_name(t)) => {
                Some(*value)
            }
            Item::Field { name, value, .. } if is_config_name(name) => *value,
            _ => continue,
        };
        match config {
            Some(Expr::Call(call)) => {
                if call.arguments.keywords.iter().any(|k| k.arg.is_none())
                    || !call.arguments.args.is_empty()
                {
                    return true;
                }
            }
            Some(Expr::Dict(dict)) => {
                if dict.items.iter().any(|item| item.key.is_none()) {
                    return true;
                }
            }
            _ => return true,
        }
    }
    false
}

/// `_set_pydantic_names`: the validation aliases and the `extra` setting of a model.
fn set_pydantic_names(info: &mut TypeInfo, nodes: &[&ClassNode<'_>], source: &Source) {
    let mut options: Options<'_> = Dict::new();
    for node in nodes {
        options.update(&config_options(node, None, source));
    }
    let models: HashSet<&str> = nodes.iter().map(|node| node.name.as_str()).collect();
    info.allows_extra = nodes
        .iter()
        .any(|node| allows_extra(node, &options, &models));
    let by_name = is_const_true(options.get("populate_by_name").copied())
        || is_const_true(options.get("validate_by_name").copied());
    let generated = options
        .get("alias_generator")
        .is_some_and(|value| !matches!(value, Expr::NoneLiteral(_)));
    let mut columns: Dict<String> = Dict::new();
    let mut opaque = BTreeSet::new();
    let no_aliases = Aliases::new();
    for node in nodes {
        for (name, annotation, value) in node.fields() {
            if !takes_column(
                name,
                annotation,
                value,
                Some(Kind::BaseModel),
                &source.aliases,
            ) {
                continue;
            }
            let mut calls = field_calls(annotation, &no_aliases);
            if let Some(value @ Expr::Call(call)) = value {
                if get_name(&call.func) == Some("Field") {
                    calls.push(value);
                }
            }
            let mut keywords: Dict<&Expr> = Dict::new();
            for call in calls {
                if let Expr::Call(call) = call {
                    for keyword in &call.arguments.keywords {
                        if let Some(arg) = &keyword.arg {
                            keywords.insert(arg.as_str(), &keyword.value);
                        }
                    }
                }
            }
            let spec = keywords
                .get("validation_alias")
                .or_else(|| keywords.get("alias"));
            match spec {
                None if generated => {
                    opaque.insert(name.to_string());
                }
                None => columns.insert(name, name.to_string()),
                Some(Expr::StringLiteral(alias)) => {
                    columns.insert(crate::parse::text(alias, &source.text), name.to_string());
                    if by_name && !columns.contains(name) {
                        columns.insert(name, name.to_string());
                    }
                }
                Some(_) => {
                    opaque.insert(name.to_string());
                }
            }
        }
    }
    info.columns = columns;
    info.opaque = opaque;
}

/// `closed` / `extra_items` as written in a `TypedDict` header.
struct Openness<'a> {
    /// `closed=True`, `extra_items=ReadOnly[int]`.
    spelling: String,
    closed: bool,
    extra_items: Option<String>,
    readonly: bool,
    extra_node: Option<&'a Expr>,
}

/// `_own_openness`: the openness a class header declares; `extra_items=Never` is
/// `closed=True`.
fn own_openness<'a>(node: &ClassNode<'a>, source: &Source) -> Option<Openness<'a>> {
    let aliases = &source.aliases;
    if let Some(keyword) = node.keyword("extra_items") {
        let spelling = format!("extra_items={}", unparse(keyword, &source.text));
        let extra = unwrap_qualifiers(keyword, aliases).0;
        if is_name(extra, "Never", aliases) || is_name(extra, "NoReturn", aliases) {
            return Some(Openness {
                spelling,
                closed: true,
                extra_items: None,
                readonly: false,
                extra_node: None,
            });
        }
        let readonly =
            matches!(keyword, Expr::Subscript(sub) if is_name(&sub.value, "ReadOnly", aliases));
        return Some(Openness {
            spelling,
            closed: false,
            extra_items: Some(normalize_type_str(&unparse(extra, &source.text))),
            readonly,
            extra_node: Some(extra),
        });
    }
    match node.keyword("closed") {
        Some(Expr::BooleanLiteral(value)) => Some(Openness {
            spelling: format!("closed={}", if value.value { "True" } else { "False" }),
            closed: value.value,
            extra_items: None,
            readonly: false,
            extra_node: None,
        }),
        _ => None,
    }
}

fn extra_repr(info: &TypeInfo) -> String {
    match &info.extra_items_type {
        Some(tp) => tp.display(),
        None => info
            .extra_items
            .clone()
            .unwrap_or_else(|| "None".to_string()),
    }
}

/// `_header_errors`: PEP 728 violations of a subclass header against its closed /
/// `extra_items` bases.
fn header_errors(
    name: &str,
    node: &ClassNode<'_>,
    own: Option<&Openness<'_>>,
    own_type: Option<&Ty>,
    parents: &[Arc<TypeInfo>],
) -> Vec<String> {
    let reopen = is_const_false(node.keyword("closed"));
    let mut errors = Vec::new();
    for p in parents {
        let Some(own) = own.filter(|_| p.keyed()) else {
            continue;
        };
        let base = format!("base {}", p.owner);
        if reopen {
            let what = if p.closed {
                "is closed"
            } else {
                "has extra_items"
            };
            errors.push(format!("{name} cannot set closed=False: {base} {what}"));
        } else if p.closed && !own.closed {
            errors.push(format!(
                "{name} cannot set {}: {base} is closed",
                own.spelling
            ));
        } else if p.extra_items.is_some() && !p.extra_items_readonly {
            if own.closed
                || own.readonly
                || consistent(own_type, p.extra_items_type.as_ref()) == Some(false)
            {
                errors.push(format!(
                    "{name} cannot set {}: {base} has mutable extra_items {}",
                    own.spelling,
                    extra_repr(p)
                ));
            }
        } else if p.extra_items_readonly
            && is_subtype(own_type, p.extra_items_type.as_ref(), Promotions::Numeric) == Some(false)
        {
            errors.push(format!(
                "{name} cannot set {}: {base} has read-only extra_items {}",
                own.spelling,
                extra_repr(p)
            ));
        }
    }
    errors
}

/// `_key_errors`: PEP 728 violations of the keys `info` has beyond each closed /
/// `extra_items` base.
fn key_errors(info: &TypeInfo, parents: &[Arc<TypeInfo>]) -> Vec<String> {
    let mut errors = Vec::new();
    for p in parents.iter().filter(|p| p.keyed()) {
        for (key, _) in info.fields.iter().filter(|(key, _)| !p.has_field(key)) {
            let tp = info.field_types.get(key).and_then(Option::as_ref);
            if p.closed {
                errors.push(format!(
                    "{} cannot add key '{key}' to closed base {}",
                    info.name, p.owner
                ));
                continue;
            }
            let mut what = format!("extra_items {}", extra_repr(p));
            let fits = if p.extra_items_readonly {
                what = format!("read-only {what}");
                is_subtype(tp, p.extra_items_type.as_ref(), Promotions::Numeric)
            } else {
                if info.required.contains(key) {
                    errors.push(format!(
                        "{}.{key} must be NotRequired: base {} has {what}",
                        info.name, p.owner
                    ));
                }
                consistent(tp, p.extra_items_type.as_ref())
            };
            if fits == Some(false) {
                errors.push(format!(
                    "{}.{key} is {} but base {} has {what}",
                    info.name,
                    tp.map_or_else(|| "None".to_string(), Ty::display),
                    p.owner
                ));
            }
        }
    }
    errors
}

/// `_has_enum`: whether `tp` is an `enum.Enum` class, or has one among its arguments.
fn has_enum(tp: &Ty, version: Version) -> bool {
    match tp {
        Ty::Class(class) => {
            stdlib_class("enum.Enum", version).is_some_and(|e| is_subclass(class, &e))
        }
        Ty::Generic(_, args) | Ty::Union(args) => args.iter().any(|arg| has_enum(arg, version)),
        Ty::Never | Ty::Literal(_) | Ty::NewType(_) => false,
    }
}

/// `_ast_key_types`: the types of the fields of a class lineage (bases first).
fn key_types(
    nodes: &[&ClassNode<'_>],
    resolver: &Resolver<'_>,
    source: &Source,
    names: &dyn Names,
) -> Dict<Option<Ty>> {
    let mut found = Dict::new();
    for node in nodes {
        for (name, annotation, _) in node.fields() {
            let bare = unwrap_qualifiers(annotation, &source.aliases).0;
            found.insert(name, resolver.resolve(bare, &source.text, names));
        }
    }
    found
}

fn without_enums(types: &Dict<Option<Ty>>, version: Version) -> Dict<Option<Ty>> {
    types
        .iter()
        .map(|(name, tp)| (name, tp.clone().filter(|tp| !has_enum(tp, version))))
        .collect()
}

/// `_lineage`: `node` and its earlier-defined bases among `classes`, bases first.
fn lineage(
    node: usize,
    all: &[ClassNode<'_>],
    classes: &HashMap<&str, usize>,
    out: &mut Dict<usize>,
) {
    for name in all[node].base_names() {
        if let Some(&base) = classes.get(name) {
            if all[base].lineno < all[node].lineno {
                lineage(base, all, classes, out);
            }
        }
    }
    out.insert(all[node].name.as_str(), node);
}

/// `extract_type_definitions`: the row types of `source`, by name: `TypedDict` and
/// `NamedTuple` subclasses, dataclasses, Pydantic models and `Protocol` classes.
///
/// Names imported from the modules of `index` are read from those modules' source,
/// standard-library classes from typeshed's class graph and Pydantic's types from a table;
/// any other imported name is unknown. `module` is the dotted name of `source`.
#[allow(clippy::too_many_lines)]
pub fn extract_type_definitions(
    source: &Arc<Source>,
    index: Option<&Arc<dyn Index>>,
    module: Option<&str>,
) -> Dict<Arc<TypeInfo>> {
    let aliases = &source.aliases;
    let text: Arc<str> = Arc::from(source.text.as_str());
    let names = ModuleTypes::new(source.clone(), index.cloned(), module);

    let mut classes: Vec<ClassNode<'_>> = source
        .classes()
        .into_iter()
        .map(|node| ClassNode::of(node, source))
        .collect();
    let functional = functional_classes(source);
    if !functional.is_empty() {
        classes.extend(functional);
        classes.sort_by_key(|node| node.lineno);
    }

    let mut imported_bases: HashMap<&str, Arc<TypeInfo>> = HashMap::new();
    if let Some(index) = index {
        let local: HashSet<&str> = classes.iter().map(|node| node.name.as_str()).collect();
        for name in classes.iter().flat_map(ClassNode::base_names) {
            if !local.contains(name) && !imported_bases.contains_key(name) {
                if let Some(found) = index.row_type(source, name) {
                    imported_bases.insert(name, found.info);
                }
            }
        }
    }
    let imported_kind = |name: &str, kind: Kind| {
        imported_bases
            .get(name)
            .is_some_and(|info| info.kind == kind)
    };

    let mut recognized: Vec<(usize, Kind)> = Vec::new();
    let mut typeddict_nodes: HashMap<&str, usize> = HashMap::new();
    let mut model_nodes: HashMap<&str, usize> = HashMap::new();
    let mut dataclass_nodes: HashMap<&str, usize> = HashMap::new();
    for (position, node) in classes.iter().enumerate() {
        let kind = classify(
            node.bases.iter().copied(),
            node.decorators.iter().copied(),
            aliases,
            &|name| typeddict_nodes.contains_key(name) || imported_kind(name, Kind::TypedDict),
            &|name| model_nodes.contains_key(name) || imported_kind(name, Kind::BaseModel),
        );
        let Some(kind) = kind else {
            continue;
        };
        match kind {
            Kind::TypedDict => typeddict_nodes.insert(node.name.as_str(), position),
            Kind::BaseModel => model_nodes.insert(node.name.as_str(), position),
            Kind::Dataclass => dataclass_nodes.insert(node.name.as_str(), position),
            _ => None,
        };
        recognized.push((position, kind));
    }

    let mut result: Dict<Arc<TypeInfo>> = Dict::new();
    let mut typeddicts: HashMap<String, Arc<TypeInfo>> = HashMap::new();
    // Dataclasses and Pydantic models, whose fields their subclasses inherit.
    let mut bases: HashMap<String, Arc<TypeInfo>> = HashMap::new();
    for (name, info) in &imported_bases {
        if info.kind == Kind::TypedDict {
            typeddicts.insert((*name).to_string(), info.clone());
        } else {
            bases.insert((*name).to_string(), info.clone());
        }
    }
    // The bases from other modules each class inherits, through those defined in the file too.
    let mut imported: HashMap<String, Vec<Arc<TypeInfo>>> = HashMap::new();
    for (position, kind) in recognized {
        let node = &classes[position];
        let total = !is_const_false(node.keyword("total"));
        let (own, own_required) = own_fields(node, source, total, kind);
        let known = if kind == Kind::TypedDict {
            &typeddicts
        } else {
            &bases
        };
        let parents: Vec<Arc<TypeInfo>> = node
            .base_names()
            .filter_map(|name| known.get(name))
            .filter(|parent| parent.kind == kind)
            .cloned()
            .collect();
        let mut fields: Dict<String> = Dict::new();
        for parent in &parents {
            for (field, annotation) in &parent.fields {
                fields.insert(field.as_str(), annotation.clone());
            }
        }
        let mut required: BTreeSet<String> = parents
            .iter()
            .flat_map(|parent| parent.required.iter().cloned())
            .collect();
        for (field, annotation) in &own {
            fields.insert(field.as_str(), annotation.clone());
            required.remove(field);
        }
        required.extend(own_required);

        let nodes: Vec<&ClassNode<'_>> = match kind {
            Kind::TypedDict | Kind::BaseModel | Kind::Dataclass => {
                let of_kind = match kind {
                    Kind::TypedDict => &typeddict_nodes,
                    Kind::BaseModel => &model_nodes,
                    _ => &dataclass_nodes,
                };
                let mut found = Dict::new();
                lineage(position, &classes, of_kind, &mut found);
                found.values().map(|&index| &classes[index]).collect()
            }
            _ => vec![node],
        };
        for ancestor in &nodes {
            for (name, annotation, _) in ancestor.fields() {
                if metadata_has_default(annotation, aliases) {
                    required.remove(name);
                }
            }
        }
        let fields: Vec<(String, String)> = fields
            .iter()
            .map(|(name, annotation)| (name.to_string(), annotation.clone()))
            .collect();
        let mut info = TypeInfo {
            name: node.name.clone(),
            fields,
            kind,
            required,
            closed: false,
            extra_items: None,
            extra_items_readonly: false,
            extra_items_type: None,
            field_types: Dict::new(),
            owner: node.name.clone(),
            lineno: node.lineno,
            bases: Vec::new(),
            errors: Vec::new(),
            columns: Dict::new(),
            opaque: BTreeSet::new(),
            // A row that has a Protocol's attributes may have more.
            allows_extra: kind == Kind::Protocol,
            lax: BTreeSet::new(),
            annotations: Dict::new(),
            aliases: aliases.clone(),
            module: module.map(str::to_string),
            coerced: None,
            type_params: node.type_params.clone(),
        };
        if kind == Kind::BaseModel {
            set_pydantic_names(&mut info, &nodes, source);
        }
        let configs: Vec<Options<'_>> = nodes
            .iter()
            .filter_map(|n| pydantic_config(n, kind, source))
            .collect();
        let pydantic = !configs.is_empty();
        if pydantic {
            let mut config: Options<'_> = Dict::new();
            for options in &configs {
                config.update(options);
            }
            for n in &nodes {
                info.lax.extend(lax_fields(n, &config, source));
            }
        }
        let known_types = if pydantic {
            Known::Model
        } else {
            Known::Checker
        };
        let resolver = source.resolver(known_types);
        info.field_types = key_types(&nodes, &resolver, source, &names);
        if pydantic {
            // Pydantic validates an enum by value: a `str` column fills a `Color` field.
            info.field_types = without_enums(&info.field_types, source.version);
        }
        for n in &nodes {
            for (name, annotation, _) in n.fields() {
                info.annotations.insert(
                    name,
                    Annotation {
                        expr: unwrap_qualifiers(annotation, aliases).0.clone(),
                        source: text.clone(),
                    },
                );
            }
        }
        let inherited: Vec<Arc<TypeInfo>> = parents
            .iter()
            .flat_map(|p| {
                if imported_bases
                    .get(p.name.as_str())
                    .is_some_and(|base| Arc::ptr_eq(base, p))
                {
                    vec![p.clone()]
                } else {
                    imported.get(&p.name).cloned().unwrap_or_default()
                }
            })
            .collect();
        for parent in &inherited {
            info.field_types = parent.field_types.merged(&info.field_types);
            info.annotations = parent.annotations.merged(&info.annotations);
            info.lax.extend(parent.lax.iter().cloned());
            if kind == Kind::BaseModel {
                info.columns = parent.columns.merged(&info.columns);
                info.opaque.extend(parent.opaque.iter().cloned());
                info.allows_extra |= parent.allows_extra;
            }
        }
        if !pydantic && kind != Kind::Protocol {
            let mut coerced = key_types(&nodes, &source.resolver(Known::Model), source, &names);
            for parent in &inherited {
                coerced = parent
                    .coerced
                    .as_ref()
                    .unwrap_or(&parent.field_types)
                    .merged(&coerced);
            }
            info.coerced = Some(without_enums(&coerced, source.version));
        }
        if kind == Kind::TypedDict {
            info.bases = parents.iter().map(|p| p.name.clone()).collect();
            let own = own_openness(node, source);
            let own_type = own
                .as_ref()
                .and_then(|own| own.extra_node)
                .and_then(|extra| resolver.resolve(extra, &source.text, &names));
            let header = header_errors(&info.name, node, own.as_ref(), own_type.as_ref(), &parents);
            for error in header.iter().cloned().chain(key_errors(&info, &parents)) {
                if !info.errors.contains(&error) {
                    info.errors.push(error);
                }
            }
            // A valid header's openness, else the first closed / `extra_items` base's.
            match own.filter(|_| header.is_empty()) {
                Some(own) => {
                    info.closed = own.closed;
                    info.extra_items_readonly = own.readonly;
                    info.extra_items_type = own_type.filter(|_| own.extra_items.is_some());
                    info.extra_items = own.extra_items;
                }
                None => {
                    if let Some(base) = parents.iter().find(|p| p.keyed()) {
                        info.closed = base.closed;
                        info.extra_items.clone_from(&base.extra_items);
                        info.extra_items_readonly = base.extra_items_readonly;
                        info.extra_items_type.clone_from(&base.extra_items_type);
                        info.owner.clone_from(&base.owner);
                    }
                }
            }
        }
        let has_fields = !info.fields.is_empty();
        let info = Arc::new(info);
        imported.insert(info.name.clone(), inherited);
        if kind == Kind::TypedDict {
            typeddicts.insert(info.name.clone(), info.clone());
        } else {
            bases.insert(info.name.clone(), info.clone());
        }
        if has_fields {
            result.insert(info.name.clone(), info);
        }
    }
    result
}
