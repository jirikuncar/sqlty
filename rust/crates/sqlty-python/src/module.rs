//! What the Python passes ask of a module's syntax tree, from one parse with
//! `ruff_python_parser`, so a file that holds none of it is never parsed by Python's `ast`.
//!
//! - [`Facts::called`]: the name of every call (`f(...)`, `x.f(...)`), sorted and unique.
//! - [`Facts::from_imports`]: `(local, original)` of every `from m import original as local`.
//! - [`Facts::any_calls`]: `(offset, callee)` of each `X[T](...)` whose `T` is `typing.Any`,
//!   as `sqlty._source.ParsedSource.any_calls` finds them before matching the wrapper.

use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, Stmt};
use ruff_python_parser::{parse_expression, parse_module};
use ruff_text_size::Ranged;

const TYPING_MODULES: [&str; 2] = ["typing", "typing_extensions"];

/// See the module documentation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The name of every call, sorted and unique.
    pub called: Vec<String>,
    /// `(local, original)` of every `from m import original as local`.
    pub from_imports: Vec<(String, String)>,
    /// `(offset, callee)` of each `X[T](...)` whose `T` is `typing.Any`.
    pub any_calls: Vec<(usize, String)>,
}

/// The facts of `source`, or `None` when it is not valid Python.
///
/// # Example
///
/// ```
/// use sqlty_python::module::facts;
///
/// let found = facts("from typing import Any as A\nfrom m import run as r\nr(SQL[A]('x'))\n").unwrap();
/// assert_eq!(found.called, ["r"]);
/// assert_eq!(found.from_imports, [("A".to_string(), "Any".to_string()), ("r".to_string(), "run".to_string())]);
/// assert_eq!(found.any_calls, [(53, "SQL".to_string())]);
/// ```
pub fn facts(source: &str) -> Option<Facts> {
    let parsed = parse_module(source).ok()?;
    let module = parsed.syntax();
    let mut imports = Vec::new();
    collect_imports(&module.body, 1, &mut imports);
    imports.sort_by_key(|(depth, _)| *depth);
    let mut found = Facts::default();
    let mut typing: Vec<(String, String)> = Vec::new();
    for (_, stmt) in &imports {
        match stmt {
            Stmt::ImportFrom(from) => {
                let module_name = from
                    .module
                    .as_ref()
                    .map(ruff_python_ast::Identifier::as_str);
                let is_typing =
                    from.level == 0 && module_name.is_some_and(|m| TYPING_MODULES.contains(&m));
                for alias in &from.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                    let original = alias.name.to_string();
                    if is_typing {
                        typing.push((local.clone(), original.clone()));
                    }
                    found.from_imports.push((local, original));
                }
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    if TYPING_MODULES.contains(&alias.name.as_str()) {
                        let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                        typing.push((local, alias.name.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    let aliases = Aliases(typing);
    let mut calls = Calls {
        aliases: &aliases,
        called: Vec::new(),
        any_calls: Vec::new(),
    };
    calls.visit_body(&module.body);
    calls.called.sort_unstable();
    calls.called.dedup();
    calls.any_calls.sort_unstable();
    found.called = calls.called;
    found.any_calls = calls.any_calls;
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
struct Aliases(Vec<(String, String)>);

impl Aliases {
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
    aliases: &'a Aliases,
    called: Vec<String>,
    any_calls: Vec<(usize, String)>,
}

impl<'a> Visitor<'a> for Calls<'_> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            match call.func.as_ref() {
                Expr::Name(name) => self.called.push(name.id.to_string()),
                Expr::Attribute(attr) => self.called.push(attr.attr.to_string()),
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
    fn invalid_source_has_no_facts() {
        assert_eq!(facts("def ("), None);
    }

    #[test]
    fn opaque_base() {
        let found = facts("import typing\nf().SQL[typing.Any](1)\n").unwrap();
        assert_eq!(found.any_calls, [(14, "?.SQL".to_string())]);
    }
}
