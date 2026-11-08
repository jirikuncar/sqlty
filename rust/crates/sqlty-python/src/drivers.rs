//! Driver calls that pass a query and its parameters, as `sqlty._drivers.find_driver_calls`
//! finds them: `cursor.execute(query, params)` on a receiver the file shows is a
//! psycopg, sqlite3, aiosqlite or asyncpg connection or cursor, and the
//! `sqlty.drivers.psycopg` helpers.

use std::collections::{HashMap, HashSet};

use ruff_python_ast::visitor::{
    walk_comprehension, walk_except_handler, walk_expr, walk_stmt, Visitor,
};
use ruff_python_ast::{Comprehension, ExceptHandler, Expr, ExprCall, ExprContext, Stmt};
use ruff_text_size::Ranged;

use crate::scopes::{dotted, is_scalar, qualified, Context, Lines, Node, Scan, Scope};
pub use crate::scopes::{Key, Param};

const CONNECTIONS: [(&str, &str); 13] = [
    ("psycopg.connect", "psycopg"),
    ("psycopg.Connection", "psycopg"),
    ("psycopg.AsyncConnection", "psycopg"),
    ("psycopg.AsyncConnection.connect", "psycopg"),
    ("psycopg.Connection.connect", "psycopg"),
    ("sqlite3.connect", "sqlite3"),
    ("sqlite3.Connection", "sqlite3"),
    ("aiosqlite.connect", "aiosqlite"),
    ("aiosqlite.Connection", "aiosqlite"),
    ("asyncpg.connect", "asyncpg"),
    ("asyncpg.Connection", "asyncpg"),
    ("asyncpg.create_pool", "asyncpg"),
    ("asyncpg.Pool", "asyncpg"),
];
const CURSORS: [(&str, &str); 4] = [
    ("psycopg.Cursor", "psycopg"),
    ("psycopg.AsyncCursor", "psycopg"),
    ("sqlite3.Cursor", "sqlite3"),
    ("aiosqlite.Cursor", "aiosqlite"),
];
const BUNDLED_MODULE: &str = "sqlty.drivers.psycopg.";
fn methods(driver: &str) -> &'static [&'static str] {
    match driver {
        "psycopg" | "sqlite3" => &["execute", "executemany"],
        "aiosqlite" => &[
            "execute",
            "executemany",
            "execute_fetchall",
            "execute_insert",
        ],
        _ => &[
            "execute",
            "fetch",
            "fetchrow",
            "fetchval",
            "executemany",
            "fetchmany",
        ],
    }
}

fn any_method(name: &str) -> bool {
    ["psycopg", "aiosqlite", "asyncpg"]
        .iter()
        .any(|driver| methods(driver).contains(&name))
}

/// A recognised driver call running a known query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverCall {
    /// The 1-based line the call starts on.
    pub line: usize,
    /// The query text.
    pub query: String,
    /// The arguments for the placeholders, empty when none are passed, `None` when not spelled out.
    pub params: Option<Vec<Param>>,
    /// `psycopg`, `sqlite3`, `aiosqlite` or `asyncpg`.
    pub driver: &'static str,
    /// The type of a single value passed where the driver takes a sequence or mapping.
    pub single: Option<String>,
    /// The byte offsets of the argument the query comes from.
    pub query_span: (usize, usize),
}

/// Whether `text` can hold a driver call at all, as the Python pass decides before parsing.
pub fn may_hold(text: &str) -> bool {
    ["psycopg", "sqlite3", "aiosqlite", "asyncpg"]
        .iter()
        .any(|driver| text.contains(driver))
        && ["execute", "fetch", "scalar"]
            .iter()
            .any(|method| text.contains(method))
}

/// The driver calls of `body`, in source order.
///
/// `imported` is `ParsedSource.imported_names` as `(local, dotted)` pairs, first binding first.
pub fn find(source: &str, body: &[Stmt], imported: &HashMap<String, String>) -> Vec<DriverCall> {
    let helpers = imported.values().any(|name| {
        let dotted = format!("{name}.");
        dotted.starts_with(BUNDLED_MODULE) || BUNDLED_MODULE.starts_with(&dotted)
    });
    let scan = Scan::new(body, |call| match call.func.as_ref() {
        Expr::Attribute(attr) => {
            (any_method(attr.attr.as_str()) && matches!(*attr.value, Expr::Name(_))) || helpers
        }
        Expr::Name(_) => helpers,
        _ => false,
    });
    let mut finder = Finder {
        ctx: Context {
            imported,
            scan: &scan,
        },
        body,
        bindings: HashMap::new(),
        resolving: HashSet::new(),
        lines: None,
        source,
        found: Vec::new(),
    };
    for (call, stack) in &scan.calls {
        finder.call(call, stack);
    }
    let mut found = finder.found;
    found.sort_by_key(|call| call.query_span);
    found
}

type Binding<'a> = (Option<&'a Expr>, Option<&'a Expr>);

struct Finder<'a, 's> {
    ctx: Context<'a, 's>,
    body: &'a [Stmt],
    bindings: HashMap<Scope, HashMap<&'a str, Vec<Binding<'a>>>>,
    resolving: HashSet<(Scope, String)>,
    lines: Option<Lines>,
    source: &'s str,
    found: Vec<DriverCall>,
}

impl<'a> Finder<'a, '_> {
    fn call(&mut self, call: &'a ExprCall, stack: &[Scope]) {
        let chain: Vec<Scope> = std::iter::once(0)
            .chain(
                stack
                    .iter()
                    .copied()
                    .filter(|&s| !self.ctx.scan.is_class(s)),
            )
            .collect();
        if let Some(bundled) = self.bundled(call) {
            for (first, arg) in bundled {
                let query = self.query_of(first, &chain);
                self.calls(call, first, arg, "psycopg", query, stack, false);
            }
            return;
        }
        let args = &call.arguments.args;
        let Expr::Attribute(func) = call.func.as_ref() else {
            return;
        };
        let attr = func.attr.as_str();
        let Expr::Name(receiver) = func.value.as_ref() else {
            return;
        };
        if !any_method(attr) {
            return;
        }
        let Some(driver) = self.driver(receiver.id.as_str(), &chain) else {
            return;
        };
        if !methods(driver).contains(&attr)
            || args.is_empty()
            || args.iter().any(|arg| matches!(arg, Expr::Starred(_)))
        {
            return;
        }
        let first = &args[0];
        let Some(query) = self.query_of(first, &chain) else {
            return;
        };
        if driver == "asyncpg" {
            let arg_lists: Vec<&[Expr]> = if matches!(attr, "executemany" | "fetchmany") {
                let rows = args.get(1).or_else(|| keyword(call, "args"));
                let rows = match rows {
                    Some(Expr::List(list)) => &list.elts,
                    Some(Expr::Tuple(tuple)) => &tuple.elts,
                    _ => return,
                };
                rows.iter()
                    .filter_map(|row| match row {
                        Expr::List(list) => Some(&list.elts[..]),
                        Expr::Tuple(tuple) => Some(&tuple.elts[..]),
                        _ => None,
                    })
                    .filter(|items| !items.iter().any(|item| matches!(item, Expr::Starred(_))))
                    .collect()
            } else {
                vec![&args[1..]]
            };
            for items in arg_lists {
                let params = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| Param {
                        key: Key::Index(i + 1),
                        kind: self.ctx.arg_type(item, stack),
                    })
                    .collect();
                self.push(call, first, query.clone(), Some(params), driver, None);
            }
            return;
        }
        let name = if driver == "psycopg" {
            "params"
        } else {
            "parameters"
        };
        let mut arg = args.get(1);
        if arg.is_none() {
            arg = keyword(call, name);
            if arg.is_none() && call.arguments.keywords.iter().any(|k| k.arg.is_none()) {
                return;
            }
        }
        if attr != "executemany" {
            self.calls(call, first, arg, driver, Some(query), stack, false);
        } else if let Some(Expr::List(_) | Expr::Tuple(_)) = arg {
            let rows = match arg {
                Some(Expr::List(list)) => &list.elts,
                Some(Expr::Tuple(tuple)) => &tuple.elts,
                _ => unreachable!(),
            };
            for row in rows {
                self.calls(
                    call,
                    first,
                    Some(row),
                    driver,
                    Some(query.clone()),
                    stack,
                    true,
                );
            }
        }
    }

    fn query_of(&mut self, first: &'a Expr, chain: &[Scope]) -> Option<String> {
        literal(Some(first)).or_else(|| match first {
            Expr::Name(name) => self.query(name.id.as_str(), chain),
            _ => None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn calls(
        &mut self,
        call: &'a ExprCall,
        first: &'a Expr,
        arg: Option<&'a Expr>,
        driver: &'static str,
        query: Option<String>,
        stack: &[Scope],
        many: bool,
    ) {
        let Some(query) = query else {
            return;
        };
        let params = match arg {
            None => Some(Vec::new()),
            Some(arg) => self.ctx.literal_params(arg, stack),
        };
        let kind = match (arg, &params) {
            (Some(arg), None) => self.ctx.arg_type(arg, stack),
            _ => None,
        };
        let sequence = matches!(driver, "sqlite3" | "aiosqlite");
        let single = kind.filter(|kind| {
            is_scalar(kind) && !(sequence && matches!(kind.as_str(), "str" | "bytes"))
        });
        if many && params.is_none() && single.is_none() {
            return;
        }
        self.push(call, first, query, params, driver, single);
    }

    fn push(
        &mut self,
        call: &ExprCall,
        first: &Expr,
        query: String,
        params: Option<Vec<Param>>,
        driver: &'static str,
        single: Option<String>,
    ) {
        let line = self.line(call.start().to_usize());
        self.found.push(DriverCall {
            line,
            query,
            params,
            driver,
            single,
            query_span: (first.start().to_usize(), first.end().to_usize()),
        });
    }

    fn line(&mut self, offset: usize) -> usize {
        let source = self.source;
        self.lines
            .get_or_insert_with(|| Lines::new(source))
            .line(offset)
    }

    /// The query and parameters of a call to a `sqlty.drivers.psycopg` helper, as `_bundled` reads them.
    fn bundled(&self, call: &'a ExprCall) -> Option<Vec<(&'a Expr, Option<&'a Expr>)>> {
        let name = qualified(dotted(&call.func), self.ctx.imported)?;
        let scalar = match name.strip_prefix(BUNDLED_MODULE)? {
            "execute" => false,
            "scalar" => true,
            _ => return None,
        };
        let args = &call.arguments.args;
        let params = keyword(call, "params");
        if args.len() < 2
            || args.iter().any(|arg| matches!(arg, Expr::Starred(_)))
            || (params.is_none() && call.arguments.keywords.iter().any(|k| k.arg.is_none()))
        {
            return Some(Vec::new());
        }
        let mut rest = &args[2..];
        if scalar && rest.len() == 1 {
            if matches!(rest[0], Expr::Name(_)) && params.is_none() {
                return Some(Vec::new());
            }
            if !matches!(rest[0], Expr::Tuple(_) | Expr::List(_) | Expr::Dict(_)) {
                rest = &[];
            }
        }
        Some(vec![(&args[1], rest.last().or(params))])
    }

    fn of(&mut self, scope: Scope) -> &HashMap<&'a str, Vec<Binding<'a>>> {
        if !self.bindings.contains_key(&scope) {
            let mut binder = Binder::default();
            match self.ctx.scan.scopes[scope] {
                Node::Module | Node::Class(_) => binder.visit_body(self.body),
                Node::Function(function) => {
                    let parameters = &function.parameters;
                    let all = parameters
                        .posonlyargs
                        .iter()
                        .chain(&parameters.args)
                        .chain(&parameters.kwonlyargs)
                        .map(|p| &p.parameter)
                        .chain(parameters.vararg.as_deref())
                        .chain(parameters.kwarg.as_deref());
                    for parameter in all {
                        binder
                            .names
                            .entry(parameter.name.as_str())
                            .or_default()
                            .push((None, parameter.annotation.as_deref()));
                    }
                    for decorator in &function.decorator_list {
                        binder.visit_decorator(decorator);
                    }
                    if let Some(type_params) = &function.type_params {
                        binder.visit_type_params(type_params);
                    }
                    binder.visit_parameters(parameters);
                    if let Some(returns) = &function.returns {
                        binder.visit_annotation(returns);
                    }
                    binder.visit_body(&function.body);
                }
            }
            self.bindings.insert(scope, binder.names);
        }
        &self.bindings[&scope]
    }

    fn lookup(&mut self, name: &str, chain: &[Scope]) -> Option<Vec<Binding<'a>>> {
        for &scope in chain.iter().rev() {
            if let Some(found) = self.of(scope).get(name) {
                return Some(found.clone());
            }
        }
        None
    }

    /// The driver of the connection or cursor `name` is, when every binding agrees.
    fn driver(&mut self, name: &str, chain: &[Scope]) -> Option<&'static str> {
        let key = (*chain.last()?, name.to_string());
        if self.resolving.contains(&key) {
            return None;
        }
        let bindings = self.lookup(name, chain)?;
        self.resolving.insert(key.clone());
        let drivers: HashSet<Option<&'static str>> = bindings
            .iter()
            .map(|&(value, annotation)| self.kind(value, annotation, chain))
            .collect();
        self.resolving.remove(&key);
        if drivers.len() == 1 {
            drivers.into_iter().next().flatten()
        } else {
            None
        }
    }

    fn kind(
        &mut self,
        value: Option<&'a Expr>,
        annotation: Option<&'a Expr>,
        chain: &[Scope],
    ) -> Option<&'static str> {
        if let Some(annotation) = annotation {
            if let Some(name) = qualified(dotted(annotation), self.ctx.imported) {
                if let Some((_, driver)) = CONNECTIONS
                    .iter()
                    .chain(&CURSORS)
                    .find(|(known, _)| *known == name)
                {
                    return Some(driver);
                }
            }
        }
        let mut value = value?;
        while let Expr::Await(awaited) = value {
            value = &awaited.value;
        }
        let Expr::Call(call) = value else {
            return None;
        };
        if let Some(name) = qualified(dotted(&call.func), self.ctx.imported) {
            if let Some((_, driver)) = CONNECTIONS.iter().find(|(known, _)| *known == name) {
                return Some(driver);
            }
        }
        match call.func.as_ref() {
            Expr::Attribute(attr) if attr.attr.as_str() == "cursor" => match attr.value.as_ref() {
                Expr::Name(base) => self.driver(base.id.as_str(), chain),
                _ => None,
            },
            _ => None,
        }
    }

    /// The query literal `name` is bound to, when it has exactly one binding.
    fn query(&mut self, name: &str, chain: &[Scope]) -> Option<String> {
        match self.lookup(name, chain)?.as_slice() {
            [(value, _)] => literal(*value),
            _ => None,
        }
    }
}

fn keyword<'a>(call: &'a ExprCall, name: &str) -> Option<&'a Expr> {
    call.arguments
        .keywords
        .iter()
        .find(|k| k.arg.as_ref().is_some_and(|arg| arg.as_str() == name))
        .map(|k| &k.value)
}

/// The text of a string literal, possibly wrapped as `SQL[T]("...")`.
fn literal(expr: Option<&Expr>) -> Option<String> {
    match expr? {
        Expr::Call(call) if call.arguments.args.len() == 1 => match call.func.as_ref() {
            Expr::Subscript(sub)
                if matches!(dotted(&sub.value).as_deref(), Some("SQL" | "sqlty.SQL")) =>
            {
                literal(call.arguments.args.first())
            }
            _ => None,
        },
        Expr::StringLiteral(text) => Some(text.value.to_str().to_string()),
        _ => None,
    }
}

/// What names a scope binds, as `_Scopes._of` reads them: `(value, annotation)`, both
/// `None` when unknown.
#[derive(Default)]
struct Binder<'a> {
    names: HashMap<&'a str, Vec<Binding<'a>>>,
}

impl<'a> Binder<'a> {
    fn bind(&mut self, target: &'a Expr, binding: Binding<'a>) {
        if let Expr::Name(name) = target {
            self.names
                .entry(name.id.as_str())
                .or_default()
                .push(binding);
        } else {
            let mut stores = Stores(Vec::new());
            stores.visit_expr(target);
            for name in stores.0 {
                self.names.entry(name).or_default().push((None, None));
            }
        }
    }
}

impl<'a> Visitor<'a> for Binder<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                self.names
                    .entry(node.name.as_str())
                    .or_default()
                    .push((None, None));
                return;
            }
            Stmt::ClassDef(node) => {
                self.names
                    .entry(node.name.as_str())
                    .or_default()
                    .push((None, None));
                return;
            }
            Stmt::Assign(node) => {
                for target in &node.targets {
                    let binding = if matches!(target, Expr::Name(_)) {
                        (Some(node.value.as_ref()), None)
                    } else {
                        (None, None)
                    };
                    self.bind(target, binding);
                }
            }
            Stmt::AnnAssign(node) => {
                self.bind(
                    &node.target,
                    (node.value.as_deref(), Some(&node.annotation)),
                );
            }
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(vars) = &item.optional_vars {
                        self.bind(vars, (Some(&item.context_expr), None));
                    }
                }
            }
            Stmt::For(node) => self.bind(&node.target, (None, None)),
            Stmt::AugAssign(node) => self.bind(&node.target, (None, None)),
            Stmt::Import(node) => {
                for alias in &node.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let head = local.split('.').next().unwrap_or(local);
                    self.names.entry(head).or_default().push((None, None));
                }
            }
            Stmt::ImportFrom(node) => {
                for alias in &node.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let head = local.split('.').next().unwrap_or(local);
                    self.names.entry(head).or_default().push((None, None));
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::Named(named) => self.bind(&named.target, (None, None)),
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_comprehension(&mut self, comprehension: &'a Comprehension) {
        self.bind(&comprehension.target, (None, None));
        walk_comprehension(self, comprehension);
    }

    fn visit_except_handler(&mut self, handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(node) = handler;
        if let Some(name) = &node.name {
            self.names
                .entry(name.as_str())
                .or_default()
                .push((None, None));
        }
        walk_except_handler(self, handler);
    }
}

/// The names a target stores to.
struct Stores<'a>(Vec<&'a str>);

impl<'a> Visitor<'a> for Stores<'a> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            if matches!(name.ctx, ExprContext::Store) {
                self.0.push(name.id.as_str());
            }
        }
        walk_expr(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::facts;

    fn calls(source: &str) -> Vec<DriverCall> {
        facts(source).unwrap().driver_calls
    }

    #[test]
    fn sqlite_tuple() {
        let found =
            calls("import sqlite3\nconn = sqlite3.connect('db')\nconn.execute('SELECT ?', (1,))\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 3);
        assert_eq!(
            found[0].params,
            Some(vec![Param {
                key: Key::Index(1),
                kind: Some("int".into())
            }])
        );
    }

    #[test]
    fn cursor_of_awaited_connection() {
        let found = calls(
            "import psycopg\nasync def f():\n    conn = await psycopg.AsyncConnection.connect('')\n    cur = conn.cursor()\n    q = 'SELECT %(a)s'\n    await cur.execute(q, {'a': -1.5})\n",
        );
        assert_eq!(found[0].driver, "psycopg");
        assert_eq!(found[0].query, "SELECT %(a)s");
        assert_eq!(
            found[0].params,
            Some(vec![Param {
                key: Key::Name("a".into()),
                kind: Some("float".into())
            }])
        );
    }

    #[test]
    fn rebound_receiver_is_unknown() {
        assert!(calls(
            "import sqlite3\nc = sqlite3.connect('')\nfor c in x: pass\nc.execute('SELECT 1')\n"
        )
        .is_empty());
    }
}
