//! `_ast_type`: an annotation resolved to a type, from source alone.

use std::collections::HashMap;
use std::sync::Arc;

use ruff_python_ast::{Expr, Number, Operator};

use super::pydantic::Known;
use super::stdlib::{stdlib_class, Version};
use super::ty::{builtins, py_strip, Class, Lit, Ty};

pub const TYPING_MODULES: [&str; 2] = ["typing", "typing_extensions"];
pub const QUALIFIERS: [&str; 3] = ["ReadOnly", "Required", "NotRequired"];

/// What a name stands for: the value of an alias, read in `source`, or a type.
pub enum Found<'a> {
    Alias(&'a Expr, &'a str),
    Type(Ty),
}

/// What [`Resolver::resolve`] reads a name through (`X` or `mod.X`).
pub trait Names {
    fn get(&self, name: &str) -> Option<Found<'_>>;
}

/// No names.
pub struct NoNames;

impl Names for NoNames {
    fn get(&self, _: &str) -> Option<Found<'_>> {
        None
    }
}

/// `names` without `hidden`, so an alias that refers to itself is unknown.
struct Hiding<'a> {
    names: &'a dyn Names,
    hidden: &'a str,
}

impl Names for Hiding<'_> {
    fn get(&self, name: &str) -> Option<Found<'_>> {
        if name == self.hidden {
            None
        } else {
            self.names.get(name)
        }
    }
}

/// The module an annotation is read in.
#[derive(Clone, Copy)]
pub struct Resolver<'a> {
    /// Local names of `typing` and its members, to the originals.
    pub aliases: &'a HashMap<String, String>,
    /// `ParsedSource.imported_names`; without it, qualified names are unknown.
    pub imports: Option<&'a HashMap<String, String>>,
    pub known: Known,
    pub version: Version,
}

fn scalar(name: &str) -> Option<Ty> {
    let b = builtins();
    let class = |c: &Arc<Class>| Some(Ty::Class(c.clone()));
    match name {
        "int" => class(&b.int),
        "float" => class(&b.float),
        "complex" => class(&b.complex),
        "str" => class(&b.str),
        "bytes" => class(&b.bytes),
        "bool" => class(&b.bool),
        "object" => class(&b.object),
        "list" => class(&b.list),
        "dict" => class(&b.dict),
        "set" => class(&b.set),
        "frozenset" => class(&b.frozenset),
        "tuple" => class(&b.tuple),
        "Any" => class(&b.any),
        "Never" | "NoReturn" => Some(Ty::Never),
        _ => None,
    }
}

fn typing_generic(name: &str) -> Option<Ty> {
    let b = builtins();
    Some(Ty::Class(
        match name {
            "List" => &b.list,
            "Dict" => &b.dict,
            "Set" => &b.set,
            "FrozenSet" => &b.frozenset,
            "Tuple" => &b.tuple,
            _ => return None,
        }
        .clone(),
    ))
}

fn is_generic(class: &Arc<Class>) -> bool {
    let b = builtins();
    [&b.list, &b.dict, &b.set, &b.frozenset, &b.tuple]
        .iter()
        .any(|generic| Arc::ptr_eq(generic, class))
}

/// `_get_name`: the simple name of an expression.
pub fn get_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str()),
        Expr::Attribute(attr) => Some(attr.attr.as_str()),
        Expr::Call(call) => get_name(&call.func),
        _ => None,
    }
}

/// `_is_name`.
pub fn is_name(expr: &Expr, name: &str, aliases: &HashMap<String, String>) -> bool {
    get_name(expr).is_some_and(|n| aliases.get(n).map_or(n, String::as_str) == name)
}

/// `_qualname`: the dotted name `expr` spells, its first part resolved through `imports`.
pub fn qualname(expr: &Expr, imports: Option<&HashMap<String, String>>) -> Option<String> {
    match expr {
        Expr::Name(name) => {
            let id = name.id.as_str();
            Some(
                imports
                    .and_then(|imports| imports.get(id))
                    .map_or(id, String::as_str)
                    .to_string(),
            )
        }
        Expr::Attribute(attr) => {
            let base = qualname(&attr.value, imports).filter(|base| !base.is_empty())?;
            Some(format!("{base}.{}", attr.attr.as_str()))
        }
        _ => None,
    }
}

/// The items of a subscript: those of a tuple, else the slice itself.
pub fn slice_items(slice: &Expr) -> &[Expr] {
    match slice {
        Expr::Tuple(tuple) => &tuple.elts,
        other => std::slice::from_ref(other),
    }
}

/// `_unwrap_qualifiers_ast`: the annotation without `ReadOnly` / `Required` /
/// `NotRequired`, and whether it is `Required` (true) or `NotRequired` (false).
pub fn unwrap_qualifiers<'e>(
    mut annotation: &'e Expr,
    aliases: &HashMap<String, String>,
) -> (&'e Expr, Option<bool>) {
    let mut required = None;
    while let Expr::Subscript(sub) = annotation {
        let Some(name) = get_name(&sub.value) else {
            break;
        };
        let name = aliases.get(name).map_or(name, String::as_str);
        if !QUALIFIERS.contains(&name) {
            break;
        }
        if name != "ReadOnly" {
            required = Some(name == "Required");
        }
        annotation = &sub.slice;
    }
    (annotation, required)
}

/// The value of a `Literal[...]` item: a `str` or an `int` constant.
fn literal_value(expr: &Expr, source: &str) -> Option<Lit> {
    match expr {
        Expr::StringLiteral(text) => Some(Lit::Str(crate::parse::text(text, source))),
        Expr::BooleanLiteral(value) => Some(Lit::Bool(value.value)),
        Expr::NumberLiteral(number) if matches!(number.value, Number::Int(_)) => {
            Some(Lit::Int(crate::unparse::unparse(expr, source)))
        }
        _ => None,
    }
}

impl<'a> Resolver<'a> {
    /// `_ast_type` of `node`, an expression of `source`; `None` if unknown.
    #[recursive::recursive]
    pub fn resolve(&self, node: &Expr, source: &str, names: &dyn Names) -> Option<Ty> {
        match node {
            Expr::NoneLiteral(_) => return Some(Ty::none()),
            Expr::StringLiteral(text) => {
                let value = crate::parse::text(text, source);
                let inner = py_strip(&value);
                let parsed = crate::parse::expression(inner)?;
                return self.resolve(&parsed.syntax().body, inner, names);
            }
            _ => {}
        }
        let imports = self.imports.filter(|imports| !imports.is_empty());
        if imports.is_some() {
            if let Some(name) = qualname(node, imports).filter(|name| !name.is_empty()) {
                if let Some(tp) = self.known.get(&name) {
                    return Some(tp.clone());
                }
                if let Some(class) = stdlib_class(&name, self.version) {
                    return Some(Ty::Class(class));
                }
            }
            let callee = match node {
                Expr::Call(call) => Some((&call.func, "()")),
                Expr::Subscript(sub) => Some((&sub.value, "[]")),
                _ => None,
            };
            if let Some((callee, suffix)) = callee {
                if let Some(name) = qualname(callee, imports).filter(|name| !name.is_empty()) {
                    if let Some(tp) = self.known.get(&format!("{name}{suffix}")) {
                        return Some(tp.clone());
                    }
                }
            }
        }
        if matches!(node, Expr::Name(_) | Expr::Attribute(_)) {
            let name = qualname(node, None).unwrap_or_default();
            if let Some(Found::Type(found)) = names.get(&name) {
                return Some(found);
            }
        }
        match node {
            Expr::Name(name) => {
                let id = name.id.as_str();
                if let Some(found) = names.get(id) {
                    return match found {
                        Found::Alias(alias, alias_source) => {
                            let hiding = Hiding { names, hidden: id };
                            self.resolve(alias, alias_source, &hiding)
                        }
                        // Only where `names` answers the same name differently each time.
                        Found::Type(found) => Some(found),
                    };
                }
                match self.aliases.get(id) {
                    Some(original) => scalar(original).or_else(|| typing_generic(original)),
                    None => scalar(id),
                }
            }
            Expr::Attribute(attr) => {
                let Expr::Name(base) = attr.value.as_ref() else {
                    return None;
                };
                let base = base.id.as_str();
                let module = self.aliases.get(base).map_or(base, String::as_str);
                if !TYPING_MODULES.contains(&module) {
                    return None;
                }
                scalar(attr.attr.as_str()).or_else(|| typing_generic(attr.attr.as_str()))
            }
            Expr::Subscript(sub) => {
                if is_name(&sub.value, "Annotated", self.aliases) {
                    return self.resolve(slice_items(&sub.slice).first()?, source, names);
                }
                if is_name(&sub.value, "Literal", self.aliases) {
                    let values: Option<Vec<Lit>> = slice_items(&sub.slice)
                        .iter()
                        .map(|item| literal_value(item, source))
                        .collect();
                    return Some(Ty::literal(values?));
                }
                if let Some(Ty::Class(origin)) = self.resolve(&sub.value, source, names) {
                    if is_generic(&origin) {
                        let args = slice_items(&sub.slice)
                            .iter()
                            .map(|arg| self.resolve(arg, source, names).unwrap_or_else(Ty::any))
                            .collect();
                        return Some(Ty::Generic(origin, args));
                    }
                }
                let none = Expr::NoneLiteral(ruff_python_ast::ExprNoneLiteral::default());
                if is_name(&sub.value, "Optional", self.aliases) {
                    self.union([sub.slice.as_ref(), &none], source, names)
                } else if is_name(&sub.value, "Union", self.aliases) {
                    self.union(slice_items(&sub.slice), source, names)
                } else {
                    None
                }
            }
            Expr::BinOp(op) if op.op == Operator::BitOr => {
                self.union([op.left.as_ref(), op.right.as_ref()], source, names)
            }
            _ => None,
        }
    }

    fn union<'e>(
        &self,
        members: impl IntoIterator<Item = &'e Expr>,
        source: &str,
        names: &dyn Names,
    ) -> Option<Ty> {
        // Every member is resolved, as each may fill a cache of `names`.
        let resolved: Vec<Option<Ty>> = members
            .into_iter()
            .map(|member| self.resolve(member, source, names))
            .collect();
        Ty::union(resolved.into_iter().collect::<Option<Vec<Ty>>>()?)
    }

    /// The same module read with other `known` types.
    #[must_use]
    pub fn with(self, known: Known) -> Resolver<'a> {
        Resolver { known, ..self }
    }
}

/// `_annotation_type`: an annotation known only as text, without its module's imports.
pub fn annotation_type(annotation: &str, version: Version) -> Option<Ty> {
    static GENERICS: std::sync::LazyLock<HashMap<String, String>> =
        std::sync::LazyLock::new(|| {
            ["List", "Dict", "Set", "FrozenSet", "Tuple"]
                .iter()
                .map(|name| ((*name).to_string(), (*name).to_string()))
                .collect()
        });
    let parsed = crate::parse::expression(annotation)?;
    let resolver = Resolver {
        aliases: &GENERICS,
        imports: None,
        known: Known::Imported,
        version,
    };
    resolver.resolve(&parsed.syntax().body, annotation, &NoNames)
}
