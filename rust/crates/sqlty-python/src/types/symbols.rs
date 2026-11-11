//! The names the annotations of a module go through, read from source.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{ExceptHandler, Expr, ExprContext, ModModule, Operator, Stmt, StmtClassDef};
use ruff_python_parser::Parsed;

use super::extract::classify;
use super::info::{Dict, TypeInfo};
use super::pydantic::Known;
use super::resolve::{is_name, Found, Names, Resolver, TYPING_MODULES};
use super::stdlib::Version;
use super::ty::{Class, NewType, Ty};
use crate::module::Imports;
use crate::scopes::Lines;

/// A module read once: its tree and what its imports name.
pub struct Source {
    pub text: String,
    pub parsed: Parsed<ModModule>,
    pub lines: Lines,
    /// `ParsedSource.typing_aliases`.
    pub aliases: Arc<HashMap<String, String>>,
    /// `ParsedSource.imported_names`.
    pub imports: HashMap<String, String>,
    pub version: Version,
}

impl Source {
    /// `text` parsed, or `None` where Rust does not read it (see [`crate::parse`]).
    pub fn new(text: &str, version: Version) -> Option<Source> {
        let parsed = crate::parse::module(text)?;
        let imports = Imports::of(&parsed.syntax().body);
        Some(Source {
            text: text.to_string(),
            lines: Lines::new(text),
            aliases: Arc::new(imports.typing.into_iter().collect()),
            imports: imports.imported,
            parsed,
            version,
        })
    }

    pub fn body(&self) -> &[Stmt] {
        &self.parsed.syntax().body
    }

    /// The line of the node at `offset`, as `ast` numbers it.
    pub fn line(&self, offset: ruff_text_size::TextSize) -> usize {
        self.lines.line(offset.to_usize())
    }

    /// `ParsedSource.classes`: every class definition, at any depth, in `ast.walk` order.
    pub fn classes(&self) -> Vec<&StmtClassDef> {
        let mut found = Vec::new();
        collect_classes(self.body(), 1, &mut found);
        found.sort_by_key(|(depth, _)| *depth);
        found.into_iter().map(|(_, class)| class).collect()
    }

    /// `ParsedSource.line_span`: the byte offsets of the 1-based `line` without its
    /// indentation and line break.
    pub fn line_span(&self, line: usize) -> (usize, usize) {
        let len = self.text.len();
        let lines =
            self.lines.count() + usize::from(self.lines.start(self.lines.count(), len) != len);
        if line == 0 || line >= lines {
            return (len, len);
        }
        let start = self.lines.start(line, len);
        let text = &self.text.as_bytes()[start..self.lines.start(line + 1, len)];
        let content = text.len()
            - text
                .iter()
                .rev()
                .take_while(|b| matches!(b, b'\r' | b'\n'))
                .count();
        let indent = text[..content]
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
            .count();
        (start + indent, start + content)
    }

    /// `ParsedSource.type_aliases`: the module-level aliases of row types: `type Row = ...`,
    /// `Row: TypeAlias = ...`, and `Row = tuple[...]` or `Row = OtherRow`.
    pub fn type_aliases(&self) -> HashMap<&str, &Expr> {
        let mut aliases = HashMap::new();
        for stmt in self.body() {
            match stmt {
                Stmt::TypeAlias(node) => {
                    if let (Expr::Name(name), None) = (node.name.as_ref(), &node.type_params) {
                        aliases.insert(name.id.as_str(), node.value.as_ref());
                    }
                }
                Stmt::AnnAssign(node) => {
                    if let (Expr::Name(name), Some(value)) = (node.target.as_ref(), &node.value) {
                        if self.typing_name(&node.annotation) == Some("TypeAlias") {
                            aliases.insert(name.id.as_str(), value.as_ref());
                        }
                    }
                }
                Stmt::Assign(node) => {
                    let [Expr::Name(name)] = node.targets.as_slice() else {
                        continue;
                    };
                    let row = match node.value.as_ref() {
                        Expr::Name(_) | Expr::Attribute(_) => true,
                        Expr::Subscript(sub) => matches!(
                            crate::unparse::unparse(&sub.value, &self.text).as_str(),
                            "tuple" | "typing.Tuple" | "Tuple"
                        ),
                        _ => false,
                    };
                    if row {
                        aliases.insert(name.id.as_str(), node.value.as_ref());
                    }
                }
                _ => {}
            }
        }
        aliases
    }

    /// `_typing_name`: the `typing` member `expr` names, if any.
    pub fn typing_name<'e>(&'e self, expr: &'e Expr) -> Option<&'e str> {
        match expr {
            Expr::Name(name) => self.aliases.get(name.id.as_str()).map(String::as_str),
            Expr::Attribute(attr) => match attr.value.as_ref() {
                Expr::Name(base)
                    if self
                        .aliases
                        .get(base.id.as_str())
                        .is_some_and(|module| TYPING_MODULES.contains(&module.as_str())) =>
                {
                    Some(attr.attr.as_str())
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn resolver(&self, known: Known) -> Resolver<'_> {
        Resolver {
            aliases: &self.aliases,
            imports: Some(&self.imports),
            known,
            version: self.version,
        }
    }
}

fn collect_classes<'a>(body: &'a [Stmt], depth: usize, out: &mut Vec<(usize, &'a StmtClassDef)>) {
    for stmt in body {
        match stmt {
            Stmt::ClassDef(node) => {
                out.push((depth, node));
                collect_classes(&node.body, depth + 1, out);
            }
            Stmt::FunctionDef(node) => collect_classes(&node.body, depth + 1, out),
            Stmt::With(node) => collect_classes(&node.body, depth + 1, out),
            Stmt::For(node) => {
                collect_classes(&node.body, depth + 1, out);
                collect_classes(&node.orelse, depth + 1, out);
            }
            Stmt::While(node) => {
                collect_classes(&node.body, depth + 1, out);
                collect_classes(&node.orelse, depth + 1, out);
            }
            Stmt::If(node) => {
                collect_classes(&node.body, depth + 1, out);
                // `ast` nests each `elif` in the `orelse` of the one before it.
                for (index, clause) in node.elif_else_clauses.iter().enumerate() {
                    let nested = depth + 1 + index + usize::from(clause.test.is_some());
                    collect_classes(&clause.body, nested, out);
                }
            }
            Stmt::Try(node) => {
                collect_classes(&node.body, depth + 1, out);
                for ExceptHandler::ExceptHandler(handler) in &node.handlers {
                    collect_classes(&handler.body, depth + 2, out);
                }
                collect_classes(&node.orelse, depth + 1, out);
                collect_classes(&node.finalbody, depth + 1, out);
            }
            Stmt::Match(node) => {
                for case in &node.cases {
                    collect_classes(&case.body, depth + 2, out);
                }
            }
            _ => {}
        }
    }
}

/// The modules of the checked paths, which names imported from them are read from.
pub trait Index: Send + Sync {
    /// The type `NAME` or `module.NAME` that `source` imports from a checked module.
    fn type_of(&self, source: &Source, dotted: &str) -> Option<Ty>;
    /// The row type `NAME` or `module.NAME` that `source` imports from a checked module.
    fn row_type(&self, source: &Source, dotted: &str) -> Option<RowType>;
}

/// A row type of another module, with the row types of that module, which its nested types
/// refer to.
#[derive(Clone)]
pub struct RowType {
    pub info: Arc<TypeInfo>,
    pub defs: Arc<Dict<Arc<TypeInfo>>>,
}

/// Whether `expr` is made of what a type is spelled with: names, subscripts, `|`, constants.
fn is_type_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Name(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => true,
        Expr::Attribute(attr) => is_type_expr(&attr.value),
        Expr::Subscript(sub) => is_type_expr(&sub.value) && is_type_expr(&sub.slice),
        Expr::Tuple(tuple) => tuple.elts.iter().all(is_type_expr),
        Expr::BinOp(op) => {
            op.op == Operator::BitOr && is_type_expr(&op.left) && is_type_expr(&op.right)
        }
        _ => false,
    }
}

/// `_bound_names`: how often each name is bound by a module-level statement; a function
/// body nested in it binds none.
struct Bound<'a> {
    counts: HashMap<&'a str, usize>,
}

impl<'a> Visitor<'a> for Bound<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if !matches!(stmt, Stmt::FunctionDef(_)) {
            walk_stmt(self, stmt);
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            if name.ctx == ExprContext::Store {
                *self.counts.entry(name.id.as_str()).or_default() += 1;
            }
        }
        walk_expr(self, expr);
    }
}

fn bound_names(body: &[Stmt]) -> HashMap<&str, usize> {
    let mut bound = Bound {
        counts: HashMap::new(),
    };
    for stmt in body {
        match stmt {
            Stmt::FunctionDef(node) => *bound.counts.entry(node.name.as_str()).or_default() += 1,
            Stmt::ClassDef(node) => *bound.counts.entry(node.name.as_str()).or_default() += 1,
            Stmt::Import(node) => {
                for alias in &node.names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let root = name.split('.').next().unwrap_or(name);
                    *bound.counts.entry(root).or_default() += 1;
                }
            }
            Stmt::ImportFrom(node) => {
                for alias in &node.names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let root = name.split('.').next().unwrap_or(name);
                    *bound.counts.entry(root).or_default() += 1;
                }
            }
            other => walk_stmt(&mut bound, other),
        }
    }
    bound.counts
}

const PYDANTIC_HOOKS: [&str; 2] = ["__get_pydantic_core_schema__", "__get_validators__"];

/// `_plain_class`: whether a class is known by its bases alone.
fn plain_class(node: &StmtClassDef, aliases: &HashMap<String, String>) -> bool {
    node.decorator_list.is_empty()
        && node
            .arguments
            .as_deref()
            .is_none_or(|arguments| arguments.keywords.is_empty())
        && node.type_params.is_none()
        && classify(
            node.bases(),
            node.decorator_list.iter().map(|d| &d.expression),
            aliases,
            &|_| false,
            &|_| false,
        )
        .is_none()
        && !node.body.iter().any(
            |item| matches!(item, Stmt::FunctionDef(f) if PYDANTIC_HOOKS.contains(&f.name.as_str())),
        )
}

/// The statement of a module-level name a type can go through.
#[derive(Clone, Copy)]
enum Local {
    Alias(usize),
    NewType(usize),
    Class(usize),
}

/// `_module_types`: the aliases, `NewType`s and plain classes of a module, by name; a name
/// bound more than once, or a generic `type X[T] = ...`, is left out.
fn module_types(body: &[Stmt], aliases: &HashMap<String, String>) -> HashMap<String, Local> {
    let mut found: HashMap<String, Local> = HashMap::new();
    for (index, stmt) in body.iter().enumerate() {
        match stmt {
            Stmt::Assign(node) => {
                let [Expr::Name(target)] = node.targets.as_slice() else {
                    continue;
                };
                if is_type_expr(&node.value) {
                    found.insert(target.id.to_string(), Local::Alias(index));
                }
            }
            Stmt::AnnAssign(node) => {
                if let (Expr::Name(target), Some(_)) = (node.target.as_ref(), &node.value) {
                    if is_name(&node.annotation, "TypeAlias", aliases) {
                        found.insert(target.id.to_string(), Local::Alias(index));
                    }
                }
            }
            Stmt::TypeAlias(node) => {
                if let (Expr::Name(target), None) = (node.name.as_ref(), &node.type_params) {
                    found.insert(target.id.to_string(), Local::Alias(index));
                }
            }
            Stmt::ClassDef(node) if plain_class(node, aliases) => {
                found.insert(node.name.to_string(), Local::Class(index));
            }
            _ => {}
        }
    }
    for (index, stmt) in body.iter().enumerate() {
        if let Some((name, _)) = newtype_assignment(stmt, aliases) {
            found.insert(name.to_string(), Local::NewType(index));
        }
    }
    let bound = bound_names(body);
    found.retain(|name, _| bound.get(name.as_str()) == Some(&1));
    found
}

/// `X = NewType("X", T)` with `T` a type expression: `X` and `T`.
fn newtype_assignment<'a>(
    stmt: &'a Stmt,
    aliases: &HashMap<String, String>,
) -> Option<(&'a str, &'a Expr)> {
    let Stmt::Assign(node) = stmt else {
        return None;
    };
    let [Expr::Name(target)] = node.targets.as_slice() else {
        return None;
    };
    let Expr::Call(call) = node.value.as_ref() else {
        return None;
    };
    let [Expr::StringLiteral(name), supertype] = &*call.arguments.args else {
        return None;
    };
    (is_name(&call.func, "NewType", aliases)
        && call.arguments.keywords.is_empty()
        && name.value.to_str() == target.id.as_str()
        && is_type_expr(supertype))
    .then_some((target.id.as_str(), supertype))
}

/// `ModuleTypes`: the aliases, `NewType`s and classes of a module, then the types it imports
/// from checked modules.
///
/// A class is a stand-in deriving from what its bases resolve to, so `class Email(str)` fits
/// a `str` field and an `int` column doesn't fit an `Email` one; with a base that is unknown,
/// it is unknown.
pub struct ModuleTypes {
    pub source: Arc<Source>,
    index: Option<Arc<dyn Index>>,
    module: String,
    local: HashMap<String, Local>,
    built: Mutex<HashMap<String, Option<Ty>>>,
    imported: Mutex<HashMap<String, Option<Ty>>>,
}

impl ModuleTypes {
    pub fn new(source: Arc<Source>, index: Option<Arc<dyn Index>>, module: Option<&str>) -> Self {
        let local = module_types(source.body(), &source.aliases);
        ModuleTypes {
            source,
            index,
            module: module.unwrap_or("__main__").to_string(),
            local,
            built: Mutex::default(),
            imported: Mutex::default(),
        }
    }

    /// Whether `name` is an alias, `NewType` or plain class of the module.
    pub fn is_local(&self, name: &str) -> bool {
        self.local.contains_key(name)
    }

    /// The type built for `name`, once; `None` while it is built, so one that refers to
    /// itself is unknown.
    fn build(&self, name: &str, build: impl FnOnce() -> Option<Ty>) -> Option<Ty> {
        let lock = || self.built.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = lock().get(name) {
            return found.clone();
        }
        lock().insert(name.to_string(), None);
        let built = build();
        lock().insert(name.to_string(), built.clone());
        built
    }
}

impl Names for ModuleTypes {
    fn get(&self, name: &str) -> Option<Found<'_>> {
        let body = self.source.body();
        let text = self.source.text.as_str();
        let resolver = self.source.resolver(Known::Imported);
        match self.local.get(name).map(|local| (*local, body)) {
            Some((Local::NewType(index), body)) => {
                let (_, supertype) = newtype_assignment(&body[index], &self.source.aliases)?;
                self.build(name, || {
                    let supertype = resolver.resolve(supertype, text, self)?;
                    Some(Ty::NewType(Arc::new(NewType {
                        name: name.to_string(),
                        module: self.module.clone(),
                        supertype,
                    })))
                })
                .map(Found::Type)
            }
            Some((Local::Class(index), body)) => {
                let Stmt::ClassDef(node) = &body[index] else {
                    return None;
                };
                self.build(name, || {
                    let bases: Vec<Option<Ty>> = node
                        .bases()
                        .iter()
                        .map(|base| resolver.resolve(base, text, self))
                        .collect();
                    let bases: Option<Vec<Arc<Class>>> = bases
                        .into_iter()
                        .map(|base| match base {
                            Some(Ty::Class(class)) => Some(class),
                            _ => None,
                        })
                        .collect();
                    Class::new(name, &self.module, &bases?).map(Ty::Class)
                })
                .map(Found::Type)
            }
            Some((Local::Alias(index), body)) => {
                let value = match &body[index] {
                    Stmt::Assign(node) => &node.value,
                    Stmt::AnnAssign(node) => node.value.as_ref()?,
                    Stmt::TypeAlias(node) => &node.value,
                    _ => return None,
                };
                Some(Found::Alias(value, text))
            }
            None => {
                let index = self.index.as_ref()?;
                let lock = || self.imported.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(found) = lock().get(name) {
                    return found.clone().map(Found::Type);
                }
                let found = index.type_of(&self.source, name);
                lock().insert(name.to_string(), found.clone());
                found.map(Found::Type)
            }
        }
    }
}
