//! What a module imports and calls, from one parse with `ruff_python_parser`.
//!
//! - [`Facts::names`] / [`Facts::attributes`]: what calls name, `f(...)` / `x.f(...)`, sorted and unique.
//! - [`Facts::imports`]: every `import` and `from ... import`, in `ast.walk` order.
//! - [`Facts::any_calls`]: `(offset, callee)` of each `X[T](...)` whose `T` is `typing.Any`,
//!   before matching the wrapper.
//! - [`Facts::typing_aliases`]: local names of `typing` objects, a later pair wins.
//! - [`Facts::driver_calls`]: see [`crate::drivers`].

use std::collections::HashMap;

use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, ExprCall, Stmt, StmtImport, StmtImportFrom};
use ruff_text_size::Ranged;

use crate::drivers::{self, DriverCall};
use crate::frozen::Table;

const TYPING_MODULES: [&str; 2] = ["typing", "typing_extensions"];

/// `import original as local`, or `from module import original as local`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The name bound: `asname`, else the name as spelled (`a.b` for `import a.b`).
    pub local: String,
    /// The name imported.
    pub original: String,
    /// The module of a `from` import, without its leading dots; `None` for `import`.
    pub module: Option<String>,
    /// The number of leading dots.
    pub level: u32,
}

/// The imports of a module.
pub struct Imports {
    /// Every alias, in `ast.walk` order.
    pub entries: Vec<Import>,
    /// `ParsedSource.imported_names`.
    pub imported: Table,
    /// `ParsedSource.typing_aliases` as pairs, a later pair wins.
    pub typing: Vec<(String, String)>,
}

impl Imports {
    /// The imports of `body`, at any depth.
    pub fn of(body: &[Stmt]) -> Self {
        let mut imports = Vec::new();
        collect_imports(body, 1, &mut imports);
        imports.sort_by_key(|(depth, _)| *depth);
        let mut imported = HashMap::new();
        let mut found = Self {
            entries: Vec::new(),
            imported: Table::default(),
            typing: Vec::new(),
        };
        for (_, stmt) in &imports {
            match stmt {
                ImportStmt::From(from) => {
                    let module_name = from.module.as_ref().map_or("", |m| m.as_str());
                    let is_typing = TYPING_MODULES.contains(&module_name);
                    for alias in &from.names {
                        let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                        let original = alias.name.to_string();
                        if is_typing {
                            found.typing.push((local.clone(), original.clone()));
                        }
                        if from.level == 0 && !module_name.is_empty() {
                            imported
                                .entry(local.clone())
                                .or_insert_with(|| format!("{module_name}.{original}"));
                        }
                        found.entries.push(Import {
                            local,
                            original,
                            module: Some(module_name.to_string()),
                            level: from.level,
                        });
                    }
                }
                ImportStmt::Plain(import) => {
                    for alias in &import.names {
                        let name = alias.name.as_str();
                        if TYPING_MODULES.contains(&name) {
                            let local = alias.asname.as_ref().unwrap_or(&alias.name).to_string();
                            found.typing.push((local, name.to_string()));
                        }
                        let root = name.split('.').next().unwrap_or(name);
                        let (local, dotted) = match &alias.asname {
                            Some(asname) => (asname.as_str(), name),
                            None => (root, root),
                        };
                        imported
                            .entry(local.to_string())
                            .or_insert_with(|| dotted.to_string());
                        found.entries.push(Import {
                            local: alias.asname.as_ref().unwrap_or(&alias.name).to_string(),
                            original: name.to_string(),
                            module: None,
                            level: 0,
                        });
                    }
                }
            }
        }
        found.imported = imported.into();
        found
    }
}

/// See the module documentation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The name of every `f(...)` call, sorted and unique.
    pub names: Vec<String>,
    /// The attribute of every `x.f(...)` call, sorted and unique.
    pub attributes: Vec<String>,
    /// Every `import` and `from ... import`, in `ast.walk` order.
    pub imports: Vec<Import>,
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
    let parsed = crate::parse::module(source)?;
    let module = parsed.syntax();
    let Imports {
        entries,
        imported,
        typing,
    } = Imports::of(&module.body);
    let mut found = Facts {
        imports: entries,
        typing_aliases: typing,
        ..Facts::default()
    };
    let table: Table = found.typing_aliases.iter().cloned().collect();
    let aliases = Aliases(&table);
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

/// `(offset, callee)` of each `X[T](...)` of `body` whose `T` is `typing.Any`, by offset;
/// `typing` is [`Imports::typing`] as a table.
pub fn any_calls(body: &[Stmt], typing: &Table) -> Vec<(usize, String)> {
    let aliases = Aliases(typing);
    let mut calls = Calls {
        aliases: &aliases,
        names: Vec::new(),
        attributes: Vec::new(),
        any_calls: Vec::new(),
    };
    calls.visit_body(body);
    calls.any_calls.sort_unstable();
    calls.any_calls
}

enum ImportStmt<'a> {
    From(&'a StmtImportFrom),
    Plain(&'a StmtImport),
}

/// Imports at any depth, with the depth `ast` gives them: one more than the node holding them.
fn collect_imports<'a>(body: &'a [Stmt], depth: usize, out: &mut Vec<(usize, ImportStmt<'a>)>) {
    for stmt in body {
        match stmt {
            Stmt::Import(node) => out.push((depth, ImportStmt::Plain(node))),
            Stmt::ImportFrom(node) => out.push((depth, ImportStmt::From(node))),
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
struct Aliases<'p>(&'p Table);

impl Aliases<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
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
            Expr::StringLiteral(text) => crate::parse::expression(text.value.to_str())
                .is_some_and(|parsed| self.is_any(parsed.syntax().body.as_ref())),
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
                _ => self.any_calls.extend(any_call(call, self.aliases)),
            }
        }
        walk_expr(self, expr);
    }
}

/// `(offset, callee)` of `call` when it is `X[T](...)` with `T` being `typing.Any`.
fn any_call(call: &ExprCall, aliases: &Aliases<'_>) -> Option<(usize, String)> {
    let Expr::Subscript(sub) = call.func.as_ref() else {
        return None;
    };
    aliases
        .is_any(&sub.slice)
        .then(|| (sub.value.start().to_usize(), callee(&sub.value)))
}

/// [`any_calls`] over calls already collected, by offset.
pub(crate) fn any_calls_among<'a>(
    calls: impl Iterator<Item = &'a ExprCall>,
    typing: &Table,
) -> Vec<(usize, String)> {
    let aliases = Aliases(typing);
    let mut found: Vec<(usize, String)> =
        calls.filter_map(|call| any_call(call, &aliases)).collect();
    found.sort_unstable();
    found
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
#[cfg_attr(coverage_nightly, coverage(off))]
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
    fn annotated_without_metadata_is_not_any() {
        let found = facts("import typing\nSQL[typing.Annotated[typing.Any]](1)\n").unwrap();
        assert_eq!(found.any_calls, []);
    }

    #[test]
    fn opaque_base() {
        let found = facts("import typing\nf().SQL[typing.Any](1)\n").unwrap();
        assert_eq!(found.any_calls, [(14, "?.SQL".to_string())]);
    }
}
