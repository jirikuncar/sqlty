//! The names the annotations of a module go through, read from source.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{walk_except_handler, walk_expr, walk_pattern, walk_stmt, Visitor};
use ruff_python_ast::{
    self as ast, ExceptHandler, Expr, ExprContext, ModModule, Operator, Stmt, StmtClassDef,
};
use ruff_text_size::{Ranged, TextRange};

use super::extract::classify;
use super::info::{Dict, TypeInfo};
use super::pydantic::Known;
use super::resolve::{is_name, Found, Names, Resolver, TYPING_MODULES};
use super::stdlib::Version;
use super::ty::{Class, NewType, Ty};
use crate::frozen::Table;
use crate::module::Imports;
use crate::scopes::Lines;

/// A module read once: its tree and what its imports name.
pub struct Source {
    pub text: Arc<str>,
    pub syntax: ModModule,
    /// The comments, where the text has a `sqlty:` one.
    pub comments: Vec<TextRange>,
    pub lines: Lines,
    /// `ParsedSource.typing_aliases`.
    pub aliases: Arc<Table>,
    /// `ParsedSource.imported_names`.
    pub imports: Table,
    /// Every import, in `ast.walk` order.
    pub entries: Vec<crate::module::Import>,
    pub version: Version,
    /// `ParsedSource.module`: the dotted name, `pkg.__init__` for a package.
    pub module: Option<String>,
    /// What the imports bind, for `module`: functions and modules by local name.
    pub(crate) targets: std::sync::OnceLock<crate::index::Targets>,
    /// See [`crate::compose::constant`].
    pub(crate) constants: std::sync::OnceLock<HashMap<String, crate::compose::Binding>>,
    /// See [`bound_names`].
    bound: std::sync::OnceLock<HashMap<String, usize>>,
    /// See [`module_types`].
    local: std::sync::OnceLock<HashMap<String, Local>>,
    /// The classes module-level names hold, see [`crate::arg_types`].
    pub(crate) kinds: std::sync::OnceLock<HashMap<String, Option<String>>>,
}

impl Source {
    /// `text` parsed, or `None` where Rust does not read it (see [`crate::parse`]).
    pub fn new(text: impl Into<Arc<str>>, version: Version) -> Option<Source> {
        let text: Arc<str> = text.into();
        if crate::parse::may_nest_surrogate(&text) {
            return None;
        }
        let parsed = crate::parse::module(&text)?;
        let comments = if text.contains("sqlty:") {
            parsed
                .tokens()
                .iter()
                .filter(|token| token.kind() == TokenKind::Comment)
                .map(Ranged::range)
                .collect()
        } else {
            Vec::new()
        };
        let syntax = parsed.into_syntax();
        let imports = Imports::of(&syntax.body);
        Some(Source {
            lines: Lines::new(&text),
            text,
            aliases: Arc::new(imports.typing.into_iter().collect()),
            imports: imports.imported,
            entries: imports.entries,
            module: None,
            targets: std::sync::OnceLock::new(),
            constants: std::sync::OnceLock::new(),
            bound: std::sync::OnceLock::new(),
            local: std::sync::OnceLock::new(),
            kinds: std::sync::OnceLock::new(),
            syntax,
            comments,
            version,
        })
    }

    /// How often the module binds `name` at module level, nested blocks included; `*` counts
    /// star imports.
    pub fn bound(&self, name: &str) -> usize {
        let bound = self.bound.get_or_init(|| {
            bound_names(self.body())
                .into_iter()
                .map(|(name, count)| (name.to_string(), count))
                .collect()
        });
        bound.get(name).copied().unwrap_or(0)
    }

    /// [`module_types`] of the module, once.
    fn local(&self) -> &HashMap<String, Local> {
        self.local
            .get_or_init(|| module_types(self.body(), &self.aliases, &self.imports))
    }

    /// The source as the module `module`.
    #[must_use]
    pub fn named(mut self, module: Option<&str>) -> Source {
        self.module = module.map(str::to_string);
        self.targets = std::sync::OnceLock::new();
        self
    }

    /// `imports_of` this module: the functions and modules its imports bind.
    pub fn targets(&self) -> &crate::index::Targets {
        self.targets
            .get_or_init(|| crate::index::imports_of(&self.entries, self.module.as_deref()))
    }

    /// [`Self::targets`] as the module `module`, or as its own one without it.
    pub fn targets_in(&self, module: Option<&str>) -> std::borrow::Cow<'_, crate::index::Targets> {
        match module {
            Some(module) if self.module.as_deref() != Some(module) => {
                std::borrow::Cow::Owned(crate::index::imports_of(&self.entries, Some(module)))
            }
            _ => std::borrow::Cow::Borrowed(self.targets()),
        }
    }

    pub fn body(&self) -> &[Stmt] {
        &self.syntax.body
    }

    /// The line of the node at `offset`, as `ast` numbers it.
    pub fn line(&self, offset: ruff_text_size::TextSize) -> usize {
        self.lines.line(offset.to_usize())
    }

    /// Every class definition, at any depth, in `ast.walk` order.
    pub fn classes(&self) -> Vec<&StmtClassDef> {
        let mut found = Vec::new();
        collect_classes(self.body(), 1, &mut found);
        found.sort_by_key(|(depth, _)| *depth);
        found.into_iter().map(|(_, class)| class).collect()
    }

    /// The byte offsets of the 1-based `line` without its
    /// indentation and line break.
    pub fn line_span(&self, line: usize) -> (usize, usize) {
        let len = self.text.len();
        let start = self.lines.start(line, len);
        let text = &self.text.as_bytes()[start..self.lines.start(line + 1, len).max(start)];
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
        let row = |value: &Expr| match value {
            Expr::Name(_) | Expr::Attribute(_) => true,
            Expr::Subscript(sub) => matches!(
                crate::unparse::unparse(&sub.value, &self.text).as_str(),
                "tuple" | "typing.Tuple" | "Tuple"
            ),
            _ => false,
        };
        for stmt in self.body() {
            let alias = match stmt {
                Stmt::TypeAlias(node) if node.type_params.is_none() => {
                    Some((node.name.as_ref(), node.value.as_ref()))
                }
                Stmt::AnnAssign(node)
                    if self.typing_name(&node.annotation) == Some("TypeAlias") =>
                {
                    node.value
                        .as_deref()
                        .map(|value| (node.target.as_ref(), value))
                }
                Stmt::Assign(node) if node.targets.len() == 1 && row(&node.value) => {
                    Some((&node.targets[0], node.value.as_ref()))
                }
                _ => None,
            };
            if let Some((Expr::Name(name), value)) = alias {
                aliases.insert(name.id.as_str(), value);
            }
        }
        aliases
    }

    /// The `typing` member `expr` names, if any.
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

/// `_bound_names`: how often each name is bound by a module-level statement, nested blocks
/// included; the bodies of functions and classes bind none.
struct Bound<'a> {
    counts: HashMap<&'a str, usize>,
}

impl<'a> Bound<'a> {
    fn bind(&mut self, name: &'a str) {
        *self.counts.entry(name).or_default() += 1;
    }
}

impl<'a> Visitor<'a> for Bound<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => self.bind(node.name.as_str()),
            Stmt::ClassDef(node) => self.bind(node.name.as_str()),
            Stmt::Import(ast::StmtImport { names, .. })
            | Stmt::ImportFrom(ast::StmtImportFrom { names, .. }) => {
                for alias in names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    self.bind(name.split('.').next().unwrap_or(name));
                }
            }
            other => walk_stmt(self, other),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            if name.ctx == ExprContext::Store {
                self.bind(name.id.as_str());
            }
        }
        walk_expr(self, expr);
    }

    fn visit_except_handler(&mut self, handler: &'a ExceptHandler) {
        let ExceptHandler::ExceptHandler(node) = handler;
        if let Some(name) = &node.name {
            self.bind(name.as_str());
        }
        walk_except_handler(self, handler);
    }

    fn visit_pattern(&mut self, pattern: &'a ast::Pattern) {
        match pattern {
            ast::Pattern::MatchAs(ast::PatternMatchAs {
                name: Some(name), ..
            })
            | ast::Pattern::MatchStar(ast::PatternMatchStar {
                name: Some(name), ..
            })
            | ast::Pattern::MatchMapping(ast::PatternMatchMapping {
                rest: Some(name), ..
            }) => {
                self.bind(name.as_str());
            }
            _ => {}
        }
        walk_pattern(self, pattern);
    }
}

/// How often each name is bound at module level, nested blocks included; `*` counts star
/// imports.
fn bound_names(body: &[Stmt]) -> HashMap<&str, usize> {
    let mut bound = Bound {
        counts: HashMap::new(),
    };
    bound.visit_body(body);
    bound.counts
}

const PYDANTIC_HOOKS: [&str; 2] = ["__get_pydantic_core_schema__", "__get_validators__"];

/// `_plain_class`: whether a class is known by its bases alone.
fn plain_class(node: &StmtClassDef, aliases: &Table) -> bool {
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
fn module_types(body: &[Stmt], aliases: &Table, imports: &Table) -> HashMap<String, Local> {
    let mut found: HashMap<String, Local> = HashMap::new();
    for (index, stmt) in body.iter().enumerate() {
        if let Some((name, _)) = type_alias(stmt, aliases) {
            found.insert(name.to_string(), Local::Alias(index));
        } else if let Stmt::ClassDef(node) = stmt {
            if plain_class(node, aliases) {
                found.insert(node.name.to_string(), Local::Class(index));
            }
        }
    }
    let bound = bound_names(body);
    let once = |name: &str| bound.get(name) == Some(&1);
    for (index, stmt) in body.iter().enumerate() {
        if let Some((name, _)) = newtype_assignment(stmt, aliases) {
            // A `NewType` defined, or imported and bound again, is not `typing.NewType`.
            let called = stmt
                .as_assign_stmt()
                .and_then(|node| crate::scopes::dotted(&node.value.as_call_expr()?.func));
            let typing = called.as_ref().is_some_and(|called| {
                matches!(
                    crate::scopes::qualified(Some(called.clone()), imports).as_deref(),
                    Some("typing.NewType" | "typing_extensions.NewType")
                ) && once(called.split('.').next().unwrap_or_default())
            });
            if typing && !bound.contains_key("*") {
                found.insert(name.to_string(), Local::NewType(index));
            }
        }
    }
    found.retain(|name, _| once(name));
    found
}

/// `X = T` with `T` a type expression, `X: TypeAlias = T` or `type X = T`: `X` and `T`.
fn type_alias<'a>(stmt: &'a Stmt, aliases: &Table) -> Option<(&'a str, &'a Expr)> {
    let (target, value): (&Expr, &Expr) = match stmt {
        Stmt::Assign(node) if node.targets.len() == 1 && is_type_expr(&node.value) => {
            (&node.targets[0], &node.value)
        }
        Stmt::AnnAssign(node) if is_name(&node.annotation, "TypeAlias", aliases) => {
            (&node.target, node.value.as_deref()?)
        }
        Stmt::TypeAlias(node) if node.type_params.is_none() => (&node.name, &node.value),
        _ => return None,
    };
    Some((target.as_name_expr()?.id.as_str(), value))
}

/// `X = NewType("X", T)` with `T` a type expression: `X` and `T`.
fn newtype_assignment<'a>(stmt: &'a Stmt, aliases: &Table) -> Option<(&'a str, &'a Expr)> {
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
pub struct ModuleTypes<S = Arc<Source>> {
    pub source: S,
    /// Weak: the index keeps the types of its modules.
    index: Option<Weak<dyn Index>>,
    module: String,
    built: Mutex<HashMap<String, Option<Ty>>>,
    imported: Mutex<HashMap<String, Option<Ty>>>,
}

impl<S: std::ops::Deref<Target = Source>> ModuleTypes<S> {
    pub fn new(source: S, index: Option<&Arc<dyn Index>>, module: Option<&str>) -> Self {
        ModuleTypes {
            source,
            index: index.map(Arc::downgrade),
            module: module
                .filter(|module| !module.is_empty())
                .unwrap_or("__main__")
                .to_string(),
            built: Mutex::default(),
            imported: Mutex::default(),
        }
    }

    /// Whether `name` is an alias, `NewType` or plain class of the module.
    pub fn is_local(&self, name: &str) -> bool {
        self.source.local().contains_key(name)
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

impl<S: std::ops::Deref<Target = Source>> Names for ModuleTypes<S> {
    fn get(&self, name: &str) -> Option<Found<'_>> {
        let body = self.source.body();
        let text = &*self.source.text;
        let resolver = self.source.resolver(Known::Imported);
        match self.source.local().get(name).map(|local| (*local, body)) {
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
                let node = body[index].as_class_def_stmt()?;
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
                let (_, value) = type_alias(&body[index], &self.source.aliases)?;
                Some(Found::Alias(value, text))
            }
            None => {
                // An import the module binds again names something else.
                if self.source.bound(name.split('.').next().unwrap_or(name)) > 1 {
                    return None;
                }
                let index = self.index.as_ref()?.upgrade()?;
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
