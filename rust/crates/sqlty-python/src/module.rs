//! What the Python passes ask of a module's syntax tree, from one parse with
//! `ruff_python_parser`, so a file that holds none of it is never parsed by Python's `ast`.
//!
//! - [`Facts::names`] / [`Facts::attributes`]: what calls name, `f(...)` / `x.f(...)`, sorted and unique.
//! - [`Facts::from_imports`]: every `from module import original as local`.
//! - [`Facts::any_calls`]: `(offset, callee)` of each `X[T](...)` whose `T` is `typing.Any`,
//!   as `sqlty._source.ParsedSource.any_calls` finds them before matching the wrapper.
//! - [`Facts::typing_aliases`]: `ParsedSource.typing_aliases`, a later pair wins.
//! - [`Facts::driver_calls`]: `sqlty._drivers.find_driver_calls`, see [`crate::drivers`].

use std::collections::HashMap;

use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, Stmt};
use ruff_python_parser::{parse_expression, parse_module};
use ruff_text_size::Ranged;

use crate::drivers::{self, DriverCall};

const TYPING_MODULES: [&str; 2] = ["typing", "typing_extensions"];

/// `from module import original as local`, `module` empty for `from . import x`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromImport {
    /// The name bound.
    pub local: String,
    /// The name imported.
    pub original: String,
    /// The module named, without its leading dots.
    pub module: String,
    /// The number of leading dots.
    pub level: u32,
}

/// See the module documentation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The name of every `f(...)` call, sorted and unique.
    pub names: Vec<String>,
    /// The attribute of every `x.f(...)` call, sorted and unique.
    pub attributes: Vec<String>,
    /// Every `from ... import`, in `ast.walk` order.
    pub from_imports: Vec<FromImport>,
    /// `(offset, callee)` of each `X[T](...)` whose `T` is `typing.Any`.
    pub any_calls: Vec<(usize, String)>,
    /// `(local, original)` names of `typing` and its members, a later pair wins.
    pub typing_aliases: Vec<(String, String)>,
    /// Driver calls running a known query, by the offset of the query.
    pub driver_calls: Vec<DriverCall>,
}

/// The facts of `source`, or `None` when it is not valid Python.
///
/// # Example
///
/// ```
/// use sqlty_python::module::facts;
///
/// let found = facts("from typing import Any as A\nfrom m import run as r\nr(SQL[A]('x'))\n").unwrap();
/// assert_eq!(found.names, ["r"]);
/// assert_eq!(found.typing_aliases, [("A".to_string(), "Any".to_string())]);
/// assert_eq!(found.any_calls, [(53, "SQL".to_string())]);
/// ```
pub fn facts(source: &str) -> Option<Facts> {
    let parsed = parse_module(source).ok()?;
    let module = parsed.syntax();
    let mut imports = Vec::new();
    collect_imports(&module.body, 1, &mut imports);
    imports.sort_by_key(|(depth, _)| *depth);
    let mut found = Facts::default();
    let mut imported: HashMap<String, String> = HashMap::new();
    for (_, stmt) in &imports {
        match stmt {
            Stmt::ImportFrom(from) => {
                let module_name = from.module.as_ref().map_or("", |m| m.as_str());
                let is_typing = TYPING_MODULES.contains(&module_name);
                for alias in &from.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                    let original = alias.name.to_string();
                    if is_typing {
                        found.typing_aliases.push((local.clone(), original.clone()));
                    }
                    if from.level == 0 && !module_name.is_empty() {
                        imported
                            .entry(local.clone())
                            .or_insert_with(|| format!("{module_name}.{original}"));
                    }
                    found.from_imports.push(FromImport {
                        local,
                        original,
                        module: module_name.to_string(),
                        level: from.level,
                    });
                }
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.name.as_str();
                    if TYPING_MODULES.contains(&name) {
                        let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                        found.typing_aliases.push((local, name.to_string()));
                    }
                    let root = name.split('.').next().unwrap_or(name);
                    let (local, dotted) = match &alias.asname {
                        Some(asname) => (asname.as_str(), name),
                        None => (root, root),
                    };
                    imported
                        .entry(local.to_string())
                        .or_insert_with(|| dotted.to_string());
                }
            }
            _ => {}
        }
    }
    let aliases = Aliases(&found.typing_aliases);
    let mut calls = Calls {
        aliases: &aliases,
        names: Vec::new(),
        attributes: Vec::new(),
        any_calls: Vec::new(),
    };
    calls.visit_body(&module.body);
    let (mut names, mut attributes, mut any_calls) =
        (calls.names, calls.attributes, calls.any_calls);
    for list in [&mut names, &mut attributes] {
        list.sort_unstable();
        list.dedup();
    }
    any_calls.sort_unstable();
    found.names = names;
    found.attributes = attributes;
    found.any_calls = any_calls;
    if drivers::may_hold(source) {
        found.driver_calls = drivers::find(source, &module.body, &imported);
    }
    Some(found)
}

/// Imports at any depth, with the depth `ast` gives them: one more than the node holding them.
fn collect_imports<'a>(body: &'a [Stmt], depth: usize, out: &mut Vec<(usize, &'a Stmt)>) {
    for stmt in body {
        match stmt {
            Stmt::Import(_) | Stmt::ImportFrom(_) => out.push((depth, stmt)),
            Stmt::FunctionDef(node) => collect_imports(&node.body, depth + 1, out),
            Stmt::ClassDef(node) => collect_imports(&node.body, depth + 1, out),
            Stmt::With(node) => collect_imports(&node.body, depth + 1, out),
            Stmt::For(node) => {
                collect_imports(&node.body, depth + 1, out);
                collect_imports(&node.orelse, depth + 1, out);
            }
            Stmt::While(node) => {
                collect_imports(&node.body, depth + 1, out);
                collect_imports(&node.orelse, depth + 1, out);
            }
            Stmt::If(node) => {
                collect_imports(&node.body, depth + 1, out);
                // `ast` nests each `elif` in the `orelse` of the one before it.
                for (index, clause) in node.elif_else_clauses.iter().enumerate() {
                    let nested = depth + 1 + index + usize::from(clause.test.is_some());
                    collect_imports(&clause.body, nested, out);
                }
            }
            Stmt::Try(node) => {
                collect_imports(&node.body, depth + 1, out);
                for ExceptHandler::ExceptHandler(handler) in &node.handlers {
                    collect_imports(&handler.body, depth + 2, out);
                }
                collect_imports(&node.orelse, depth + 1, out);
                collect_imports(&node.finalbody, depth + 1, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_imports(&case.body, depth + 2, out);
                }
            }
            _ => {}
        }
    }
}

/// Local names of `typing` and its members, to the originals; a later import wins.
struct Aliases<'p>(&'p [(String, String)]);

impl Aliases<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(local, _)| local == name)
            .map(|(_, original)| original.as_str())
    }

    /// The `typing` member `expr` names, as `_typing_name` does.
    fn member<'e>(&'e self, expr: &'e Expr) -> Option<&'e str> {
        match expr {
            Expr::Name(name) => self.get(name.id.as_str()),
            Expr::Attribute(attr) => match attr.value.as_ref() {
                Expr::Name(base)
                    if self
                        .get(base.id.as_str())
                        .is_some_and(|m| TYPING_MODULES.contains(&m)) =>
                {
                    Some(attr.attr.as_str())
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether `expr` is `typing.Any`, quoted or in `Annotated` too, as `is_any` decides.
    fn is_any(&self, expr: &Expr) -> bool {
        match expr {
            Expr::StringLiteral(text) => parse_expression(text.value.to_str())
                .is_ok_and(|parsed| self.is_any(parsed.syntax().body.as_ref())),
            Expr::Subscript(sub) if self.member(&sub.value) == Some("Annotated") => match sub
                .slice
                .as_ref()
            {
                Expr::Tuple(items) => items.elts.first().is_some_and(|first| self.is_any(first)),
                _ => false,
            },
            _ => self.member(expr) == Some("Any"),
        }
    }
}

struct Calls<'a> {
    aliases: &'a Aliases<'a>,
    names: Vec<String>,
    attributes: Vec<String>,
    any_calls: Vec<(usize, String)>,
}

impl<'a> Visitor<'a> for Calls<'_> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            match call.func.as_ref() {
                Expr::Name(name) => self.names.push(name.id.to_string()),
                Expr::Attribute(attr) => self.attributes.push(attr.attr.to_string()),
                Expr::Subscript(sub) if self.aliases.is_any(&sub.slice) => {
                    self.any_calls
                        .push((sub.value.start().to_usize(), callee(&sub.value)));
                }
                _ => {}
            }
        }
        walk_expr(self, expr);
    }
}

/// `expr` spelled as `ast.unparse` would end it: a dotted name, with `?` for a base that is not a name.
fn callee(expr: &Expr) -> String {
    let mut parts = Vec::new();
    let mut node = expr;
    while let Expr::Attribute(attr) = node {
        parts.push(attr.attr.as_str());
        node = &attr.value;
    }
    parts.push(if let Expr::Name(name) = node {
        name.id.as_str()
    } else {
        "?"
    });
    parts.reverse();
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_imports_keep_ast_order() {
        // `ast` lists the deeper import last, so its binding wins.
        let found = facts(
            "if x:\n    from typing import Any as T\nfrom typing import List as T\nq = SQL[T](1)\n",
        )
        .unwrap();
        assert_eq!(found.any_calls, [(71, "SQL".to_string())]);
    }

    #[test]
    fn relative_typing_import_aliases_too() {
        // `typing_aliases` reads `node.module` alone, as Python does.
        let found = facts("from .typing import Any\nq = SQL[Any](1)\n").unwrap();
        assert_eq!(found.any_calls, [(28, "SQL".to_string())]);
    }

    #[test]
    fn invalid_source_has_no_facts() {
        assert_eq!(facts("def ("), None);
    }

    #[test]
    fn opaque_base() {
        let found = facts("import typing\nf().SQL[typing.Any](1)\n").unwrap();
        assert_eq!(found.any_calls, [(14, "?.SQL".to_string())]);
    }
}
