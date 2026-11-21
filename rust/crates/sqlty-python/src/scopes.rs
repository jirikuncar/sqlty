//! The scopes of a module, the classes its names and `self` attributes hold, and the
//! argument types read from them.

use std::collections::{HashMap, HashSet};

use crate::frozen::Table;
use crate::types::symbols::Source;
use ruff_python_ast::visitor::{walk_except_handler, walk_expr, walk_pattern, walk_stmt, Visitor};
use ruff_python_ast::{
    self as ast, Expr, ExprCall, ExprTString, Number, Stmt, StmtClassDef, StmtFunctionDef, UnaryOp,
};
use ruff_text_size::{Ranged, TextRange, TextSize};

const BUILTINS: [&str; 5] = ["int", "str", "float", "bool", "bytes"];
const SEQUENCES: [&str; 4] = ["list", "tuple", "set", "frozenset"];
/// Classes known by the dotted name they are imported as.
pub const IMPORTED: [&str; 6] = [
    "datetime.date",
    "datetime.datetime",
    "datetime.time",
    "datetime.timedelta",
    "uuid.UUID",
    "decimal.Decimal",
];

/// Whether `kind` is the type of a single value: a builtin or an imported class.
pub fn is_scalar(kind: &str) -> bool {
    BUILTINS.contains(&kind) || IMPORTED.contains(&kind)
}

/// The parameter an argument binds, by 1-based position or by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// `%s`, `?`, `$1`: the position.
    Index(usize),
    /// `%(name)s`, `:name`: the name.
    Name(String),
}

/// An argument passed for a parameter of the query, with its type where the source states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The parameter it binds.
    pub key: Key,
    /// Spelled as the engine spells parameter types: `int`, `datetime.date`, `list[int]`, `None`.
    pub kind: Option<String>,
}

/// Scope ids: 0 is the module, then each function and class in pre-order.
pub type Scope = usize;

/// The node of a scope.
#[derive(Clone, Copy)]
pub enum Node<'a> {
    /// The module.
    Module,
    /// A function or method.
    Function(&'a StmtFunctionDef),
    /// A class.
    Class(&'a StmtClassDef),
}

/// What one walk of a module collects: its scopes, every binding of every name in them,
/// and the calls `want` keeps, each with the scopes around it (innermost last, classes
/// included).
pub struct Scan<'a> {
    /// The scopes, by id.
    pub scopes: Vec<Node<'a>>,
    /// The scope around each scope; the module is its own.
    pub parents: Vec<Scope>,
    stack: Vec<Scope>,
    /// Every binding of a name in a scope, in source order.
    bound: HashMap<(Scope, &'a str), Vec<Binding<'a>>>,
    /// Every `self.name` a method binds, by its class, with the method.
    attributes: HashMap<(Scope, &'a str), Vec<(Origin<'a>, Scope)>>,
    /// Where a comprehension or a lambda in a scope binds a name of its own.
    shadows: HashMap<(Scope, &'a str), Vec<TextRange>>,
    /// The lambdas and generator expressions of a scope: code that runs when it is
    /// called or iterated, not where it is written.
    deferred: HashMap<Scope, Vec<TextRange>>,
    /// The names each scope binds and the attributes the methods of each class bind, in
    /// order: reading those of one scope does not read those of every other.
    names: HashMap<Scope, Vec<&'a str>>,
    fields: HashMap<Scope, Vec<&'a str>>,
    /// The names a function declares `global` or `nonlocal`.
    foreign: HashSet<(Scope, &'a str)>,
    /// The calls kept, with their scopes.
    pub calls: Vec<(&'a ExprCall, Vec<Scope>)>,
    /// Every t-string, with its scopes.
    pub templates: Vec<(&'a ExprTString, Vec<Scope>)>,
}

/// Where a name gets its value. Every form that binds a name is one of these: what is
/// not read as a type is `Unknown`, so that it hides an earlier binding instead of
/// leaving it in effect.
#[derive(Clone, Copy)]
pub enum Origin<'a> {
    /// `name: T` or `name: T = value`, a parameter or a variable: the annotation and value.
    Declared(&'a Expr, Option<&'a Expr>),
    /// `name = value`.
    Value(&'a Expr),
    /// `with value as name`, `async with` when true.
    Entered(&'a Expr, bool),
    /// `class name`.
    Class(&'a StmtClassDef),
    /// `def name`.
    Function(&'a StmtFunctionDef),
    /// `import name`, `from module import name`: the statement and its alias.
    Import(&'a Stmt, &'a ast::Alias),
    /// A parameter without annotation, other than `*args` and `**kwargs`.
    Parameter,
    /// `*args`, `**kwargs`, a loop variable, an unpacked value, `name := ...`,
    /// `except ... as name`, a `match` capture, `del name`, `type name = ...`, an
    /// augmented assignment, a write through `global` or `nonlocal`.
    Unknown,
}

/// A binding of a name: written at `site`, in effect from `effect`.
#[derive(Clone, Copy)]
pub struct Binding<'a> {
    /// What it binds the name to.
    pub origin: Origin<'a>,
    /// The offset of the name bound.
    pub site: u32,
    /// The offset its value is in the name from: the end of an assignment, so that
    /// `x = f(x)` reads the earlier `x`.
    pub effect: u32,
}

#[cfg(test)]
thread_local! {
    /// The names read by this thread when the names of a scope were asked for, for the
    /// tests of cost.
    pub(crate) static READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts a name read in tests; nothing otherwise.
#[cfg_attr(
    not(test),
    expect(clippy::missing_const_for_fn, reason = "it counts in tests")
)]
fn read() {
    #[cfg(test)]
    READ.with(|read| read.set(read.get() + 1));
}

/// Keeps the ranges no other one contains, by start: expressions nest or lie apart, so
/// [`around`] finds the one around an offset without reading them all.
fn outermost(ranges: &mut Vec<TextRange>) {
    ranges.sort_by_key(|range| (range.start(), std::cmp::Reverse(range.end())));
    let mut end = TextSize::new(0);
    ranges.retain(|range| {
        let outer = range.end() > end;
        end = end.max(range.end());
        outer
    });
}

/// Where the range of `ranges` around offset `at` starts (see [`outermost`]).
fn around(ranges: &[TextRange], at: u32) -> Option<u32> {
    let after = ranges.partition_point(|range| range.start().to_u32() <= at);
    let range = ranges.get(after.checked_sub(1)?)?;
    (at < range.end().to_u32()).then(|| range.start().to_u32())
}

impl<'a> Scan<'a> {
    /// Walks `body`, keeping the calls `want` accepts.
    pub fn new(body: &'a [Stmt], want: impl FnMut(&'a ExprCall) -> bool) -> Self {
        let mut walker = Walker {
            scan: Self {
                scopes: vec![Node::Module],
                parents: vec![0],
                stack: Vec::new(),
                bound: HashMap::new(),
                attributes: HashMap::new(),
                shadows: HashMap::new(),
                deferred: HashMap::new(),
                names: HashMap::new(),
                fields: HashMap::new(),
                foreign: HashSet::new(),
                calls: Vec::new(),
                templates: Vec::new(),
            },
            want,
        };
        walker.visit_body(body);
        let mut scan = walker.scan;
        for bindings in scan.bound.values_mut() {
            bindings.sort_by_key(|binding| binding.site);
        }
        for &(scope, name) in scan.bound.keys() {
            scan.names.entry(scope).or_default().push(name);
        }
        for &(scope, name) in scan.attributes.keys() {
            scan.fields.entry(scope).or_default().push(name);
        }
        let listed = scan.names.values_mut().chain(scan.fields.values_mut());
        listed.for_each(|names| names.sort_unstable());
        scan.shadows.values_mut().for_each(outermost);
        scan.deferred.values_mut().for_each(outermost);
        scan
    }

    /// Whether `scope` is a class.
    pub fn is_class(&self, scope: Scope) -> bool {
        matches!(self.scopes[scope], Node::Class(_))
    }

    /// Every binding of `name` in `scope`, by `site`; `None` when the scope does not bind it.
    pub fn bindings<'n>(&'n self, scope: Scope, name: &'n str) -> Option<&'n [Binding<'a>]> {
        self.bound.get(&(scope, name)).map(Vec::as_slice)
    }

    /// What an argument type reads of `name` in `scope`: the class each of its
    /// assignments, annotations and parameters gives. The other forms that bind a name (a
    /// loop, an import, `del`, `+=`) say nothing of the type of an argument, and are
    /// not counted.
    fn typed<'n>(
        &'n self,
        scope: Scope,
        name: &'n str,
    ) -> impl Iterator<Item = Option<String>> + 'n {
        let bindings = self.bindings(scope, name).unwrap_or(&[]);
        bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.origin,
                    Origin::Declared(..) | Origin::Value(_) | Origin::Parameter
                )
            })
            .map(|binding| kind(binding.origin))
    }

    /// The names `scope` binds, each with its bindings.
    pub fn bound_in(&self, scope: Scope) -> impl Iterator<Item = (&'a str, &[Binding<'a>])> {
        let names = self.names.get(&scope).map_or(&[][..], Vec::as_slice);
        names.iter().filter_map(move |&name| {
            read();
            let bindings = self.bound.get(&(scope, name))?;
            Some((name, bindings.as_slice()))
        })
    }

    /// The attributes the methods of the class `scope` bind on their first parameter, each
    /// with the method that does.
    pub fn attributes_of(
        &self,
        scope: Scope,
    ) -> impl Iterator<Item = (&'a str, &[(Origin<'a>, Scope)])> {
        let names = self.fields.get(&scope).map_or(&[][..], Vec::as_slice);
        names.iter().filter_map(move |&name| {
            read();
            let bound = self.attributes.get(&(scope, name))?;
            Some((name, bound.as_slice()))
        })
    }

    /// Whether `at` is inside a comprehension or a lambda of `scope` that binds `name`.
    pub fn shadowed(&self, scope: Scope, name: &str, at: u32) -> bool {
        self.shadows
            .get(&(scope, name))
            .is_some_and(|ranges| around(ranges, at).is_some())
    }

    /// Whether a comprehension or a lambda of `scope` binds `name` anywhere.
    pub fn shadows(&self, scope: Scope, name: &str) -> bool {
        self.shadows.contains_key(&(scope, name))
    }

    /// Where the outermost lambda or generator expression of `scope` around `at` starts.
    /// It runs later: a binding the scope makes from there on may be in effect by then,
    /// as for a nested `def`.
    pub fn deferred(&self, scope: Scope, at: u32) -> Option<u32> {
        around(self.deferred.get(&scope)?, at)
    }

    fn scope(&self) -> Scope {
        self.stack.last().copied().unwrap_or(0)
    }

    /// Records that the innermost scope binds `name` at `site`, from `effect` on.
    fn bind_name(&mut self, name: &'a str, origin: Origin<'a>, site: TextSize, effect: TextSize) {
        let scope = self.scope();
        let binding = |origin| Binding {
            origin,
            site: site.to_u32(),
            effect: effect.to_u32(),
        };
        if self.foreign.contains(&(scope, name)) {
            // The write lands in the module or in a function around this one: whichever,
            // the name is unknown there.
            let around = self
                .stack
                .split_last()
                .map_or(&[][..], |(_, around)| around);
            let outer = around
                .iter()
                .copied()
                .filter(|&outer| !self.is_class(outer))
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            for outer in outer {
                self.bound
                    .entry((outer, name))
                    .or_default()
                    .push(binding(Origin::Unknown));
            }
            return;
        }
        self.bound
            .entry((scope, name))
            .or_default()
            .push(binding(origin));
    }

    /// Records what an assignment target binds: a name, an attribute of a method's first
    /// parameter, or each name of an unpacking.
    fn bind(&mut self, target: &'a Expr, origin: Origin<'a>, effect: TextSize) {
        match target {
            Expr::Name(name) => self.bind_name(name.id.as_str(), origin, name.start(), effect),
            Expr::Attribute(attr) => {
                let Expr::Name(base) = attr.value.as_ref() else {
                    return;
                };
                let [.., class, method] = self.stack[..] else {
                    return;
                };
                if let (Node::Class(_), Node::Function(function)) =
                    (self.scopes[class], self.scopes[method])
                {
                    let parameters = &function.parameters;
                    let first = parameters.posonlyargs.iter().chain(&parameters.args).next();
                    if first.is_some_and(|first| first.parameter.name.as_str() == base.id.as_str())
                    {
                        self.attributes
                            .entry((class, attr.attr.as_str()))
                            .or_default()
                            .push((origin, method));
                    }
                }
            }
            Expr::Tuple(ast::ExprTuple { elts, .. }) | Expr::List(ast::ExprList { elts, .. }) => {
                for item in elts {
                    self.bind(item, Origin::Unknown, effect);
                }
            }
            Expr::Starred(starred) => self.bind(&starred.value, Origin::Unknown, effect),
            _ => {}
        }
    }

    /// Records that `range` of the innermost scope has a `name` of its own.
    fn shadow(&mut self, target: &'a Expr, range: TextRange) {
        match target {
            Expr::Name(name) => {
                let scope = self.scope();
                self.shadows
                    .entry((scope, name.id.as_str()))
                    .or_default()
                    .push(range);
            }
            Expr::Tuple(ast::ExprTuple { elts, .. }) | Expr::List(ast::ExprList { elts, .. }) => {
                for item in elts {
                    self.shadow(item, range);
                }
            }
            Expr::Starred(starred) => self.shadow(&starred.value, range),
            _ => {}
        }
    }

    fn enter(&mut self, node: Node<'a>) -> Scope {
        let scope = self.scopes.len();
        self.scopes.push(node);
        self.parents.push(self.scope());
        scope
    }
}

struct Walker<'a, W> {
    scan: Scan<'a>,
    want: W,
}

/// The name of a type parameter.
pub fn type_parameter(param: &ast::TypeParam) -> &ast::Identifier {
    match param {
        ast::TypeParam::TypeVar(param) => &param.name,
        ast::TypeParam::ParamSpec(param) => &param.name,
        ast::TypeParam::TypeVarTuple(param) => &param.name,
    }
}

impl<'a, W> Walker<'a, W> {
    /// Binds the type parameters of a `def` or a `class` (`def f[T](x: T)`) in its scope:
    /// a name so bound is a type variable, not what the module binds it to.
    fn bind_type_params(&mut self, type_params: Option<&'a ast::TypeParams>) {
        for param in type_params.into_iter().flat_map(|params| params.iter()) {
            let name = type_parameter(param);
            self.scan
                .bind_name(name.as_str(), Origin::Unknown, name.start(), name.start());
        }
    }
}

impl<'a, W: FnMut(&'a ExprCall) -> bool> Visitor<'a> for Walker<'a, W> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                self.scan.bind_name(
                    node.name.as_str(),
                    Origin::Function(node),
                    node.name.start(),
                    node.end(),
                );
                let scope = self.scan.enter(Node::Function(node));
                self.scan.stack.push(scope);
                self.bind_type_params(node.type_params.as_deref());
                for parameter in &*node.parameters {
                    let origin = match parameter.annotation() {
                        _ if parameter.is_variadic() => Origin::Unknown,
                        Some(declared) => Origin::Declared(declared, None),
                        None => Origin::Parameter,
                    };
                    let at = parameter.name().start();
                    self.scan
                        .bind_name(parameter.name().as_str(), origin, at, at);
                }
                walk_stmt(self, stmt);
                self.scan.stack.pop();
                return;
            }
            Stmt::ClassDef(node) => {
                self.scan.bind_name(
                    node.name.as_str(),
                    Origin::Class(node),
                    node.name.start(),
                    node.end(),
                );
                let scope = self.scan.enter(Node::Class(node));
                self.scan.stack.push(scope);
                self.bind_type_params(node.type_params.as_deref());
                walk_stmt(self, stmt);
                self.scan.stack.pop();
                return;
            }
            Stmt::Assign(node) => {
                for target in &node.targets {
                    self.scan
                        .bind(target, Origin::Value(&node.value), node.end());
                }
            }
            Stmt::AnnAssign(node) => {
                let origin = match &node.value {
                    Some(value) if is_final(&node.annotation) => Origin::Value(value),
                    value => Origin::Declared(&node.annotation, value.as_deref()),
                };
                self.scan.bind(&node.target, origin, node.end());
            }
            Stmt::AugAssign(node) => self.scan.bind(&node.target, Origin::Unknown, node.end()),
            // The iterable is read before the target is bound.
            Stmt::For(node) => self
                .scan
                .bind(&node.target, Origin::Unknown, node.iter.end()),
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        self.scan.bind(
                            target,
                            Origin::Entered(&item.context_expr, node.is_async),
                            target.end(),
                        );
                    }
                }
            }
            Stmt::Delete(node) => {
                for target in &node.targets {
                    self.scan.bind(target, Origin::Unknown, node.end());
                }
            }
            Stmt::TypeAlias(node) => self.scan.bind(&node.name, Origin::Unknown, node.end()),
            Stmt::Import(node) => {
                for alias in &node.names {
                    let name = alias.name.as_str();
                    let local = match &alias.asname {
                        Some(asname) => asname.as_str(),
                        None => name.split('.').next().unwrap_or(name),
                    };
                    let origin = Origin::Import(stmt, alias);
                    self.scan
                        .bind_name(local, origin, alias.start(), node.end());
                }
            }
            Stmt::ImportFrom(node) => {
                for alias in &node.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name);
                    let origin = Origin::Import(stmt, alias);
                    self.scan
                        .bind_name(local.as_str(), origin, alias.start(), node.end());
                }
            }
            Stmt::Global(ast::StmtGlobal { names, .. })
            | Stmt::Nonlocal(ast::StmtNonlocal { names, .. }) => {
                // At the top of a module `global` changes nothing.
                if let Some(&scope) = self.scan.stack.last() {
                    for name in names {
                        self.scan.foreign.insert((scope, name.as_str()));
                    }
                }
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_except_handler(&mut self, handler: &'a ast::ExceptHandler) {
        let ast::ExceptHandler::ExceptHandler(caught) = handler;
        if let Some(name) = &caught.name {
            self.scan
                .bind_name(name.as_str(), Origin::Unknown, name.start(), name.end());
        }
        walk_except_handler(self, handler);
    }

    fn visit_pattern(&mut self, pattern: &'a ast::Pattern) {
        let name = match pattern {
            ast::Pattern::MatchAs(found) => found.name.as_ref(),
            ast::Pattern::MatchStar(found) => found.name.as_ref(),
            ast::Pattern::MatchMapping(found) => found.rest.as_ref(),
            _ => None,
        };
        if let Some(name) = name {
            self.scan
                .bind_name(name.as_str(), Origin::Unknown, name.start(), name.end());
        }
        walk_pattern(self, pattern);
    }

    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Call(call) if (self.want)(call) => {
                self.scan.calls.push((call, self.scan.stack.clone()));
            }
            Expr::TString(template) => {
                self.scan
                    .templates
                    .push((template, self.scan.stack.clone()));
            }
            Expr::Named(named) => self.scan.bind(&named.target, Origin::Unknown, named.end()),
            Expr::Lambda(lambda) => {
                let scope = self.scan.scope();
                self.scan
                    .deferred
                    .entry(scope)
                    .or_default()
                    .push(lambda.range());
                for parameter in lambda.parameters.iter().flat_map(|found| found.iter()) {
                    let scope = self.scan.scope();
                    self.scan
                        .shadows
                        .entry((scope, parameter.name().as_str()))
                        .or_default()
                        .push(lambda.range());
                }
            }
            Expr::ListComp(ast::ExprListComp { generators, .. })
            | Expr::SetComp(ast::ExprSetComp { generators, .. })
            | Expr::Generator(ast::ExprGenerator { generators, .. }) => {
                if expr.is_generator_expr() {
                    let scope = self.scan.scope();
                    self.scan
                        .deferred
                        .entry(scope)
                        .or_default()
                        .push(expr.range());
                }
                for generator in generators {
                    self.scan.shadow(&generator.target, expr.range());
                }
            }
            Expr::DictComp(ast::ExprDictComp { generators, .. }) => {
                for generator in generators {
                    self.scan.shadow(&generator.target, expr.range());
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Whether an annotation is `Final` with no type of its own.
fn is_final(annotation: &Expr) -> bool {
    matches!(
        class_name(Some(annotation)).as_deref(),
        Some("Final" | "typing.Final" | "typing_extensions.Final")
    )
}

/// The class a binding gives its name, as far as an argument type reads it: a builtin or
/// imported class named by the annotation or called for the value, or that of a literal.
fn kind(origin: Origin<'_>) -> Option<String> {
    let called = |value: &Expr| match value {
        Expr::Call(call) => class_name(Some(&call.func)),
        _ => None,
    };
    match origin {
        Origin::Declared(annotation, value) => {
            let head = match annotation {
                Expr::Subscript(subscript) => subscript.value.as_ref(),
                annotation => annotation,
            };
            let declared = if is_final(head) {
                value.and_then(literal_type).map(ToString::to_string)
            } else {
                class_name(Some(annotation))
            };
            declared.or_else(|| value.and_then(called))
        }
        Origin::Value(value) => {
            called(value).or_else(|| literal_type(value).map(ToString::to_string))
        }
        _ => None,
    }
}

/// The module that defines an imported `NAME` or `module.NAME`, and its value there.
pub type Constant<'s> = &'s dyn Fn(&str) -> Option<(&'s Source, &'s Expr)>;

/// Reads argument types in the scopes of a [`Scan`].
pub struct Context<'a, 's> {
    /// `ParsedSource.imported_names`.
    pub imported: &'s Table,
    /// The scan of the module.
    pub scan: &'s Scan<'a>,
    /// Finds the value of an imported constant, when the checked modules are known.
    pub constant: Option<Constant<'s>>,
}

impl Context<'_, '_> {
    /// The arguments a tuple or list literal, or a dict literal of string keys, passes.
    pub fn literal_params(&self, arg: &Expr, stack: &[Scope]) -> Option<Vec<Param>> {
        let items = match arg {
            Expr::Tuple(tuple) => &tuple.elts,
            Expr::List(list) => &list.elts,
            Expr::Dict(dict) => {
                let mut found = Vec::new();
                for item in &dict.items {
                    let Some(Expr::StringLiteral(key)) = &item.key else {
                        return None;
                    };
                    found.push(Param {
                        key: Key::Name(key.value.to_str().to_string()),
                        kind: self.arg_type(&item.value, stack),
                    });
                }
                return Some(found);
            }
            _ => return None,
        };
        if items.iter().any(|item| matches!(item, Expr::Starred(_))) {
            return None;
        }
        Some(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| Param {
                    key: Key::Index(i + 1),
                    kind: self.arg_type(item, stack),
                })
                .collect(),
        )
    }

    /// The type `expr` has where the source says so.
    pub fn arg_type(&self, expr: &Expr, stack: &[Scope]) -> Option<String> {
        let name = match expr {
            Expr::NoneLiteral(_) => "None",
            Expr::List(ast::ExprList { elts: items, .. })
            | Expr::Tuple(ast::ExprTuple { elts: items, .. })
            | Expr::Set(ast::ExprSet { elts: items, .. }) => {
                let types: HashSet<Option<String>> = items
                    .iter()
                    .map(|item| match item {
                        Expr::Starred(_) => None,
                        _ => self.arg_type(item, stack),
                    })
                    .collect();
                if let [Some(only)] = types.iter().collect::<Vec<_>>().as_slice() {
                    if only != "None" {
                        return Some(format!("list[{only}]"));
                    }
                }
                "list"
            }
            Expr::Name(name) => {
                let name = name.id.as_str();
                return self
                    .class(self.type_of(stack, name))
                    .or_else(|| self.imported_constant(name, name, stack));
            }
            Expr::Attribute(_) => {
                let dotted = dotted(expr)?;
                let head = dotted.split('.').next().unwrap_or_default();
                return self.imported_constant(&dotted, head, stack);
            }
            Expr::Call(call) => return self.class(dotted(&call.func)),
            expr => literal_type(expr)?,
        };
        Some(name.to_string())
    }

    /// The type of the constant `dotted` imports from a checked module, unless `head` is
    /// bound in `stack`.
    fn imported_constant(&self, dotted: &str, head: &str, stack: &[Scope]) -> Option<String> {
        let constant = self.constant?;
        if self.binds(stack, head) {
            return None;
        }
        let (source, value) = constant(dotted)?;
        let scan = Scan::new(
            if matches!(value, Expr::Name(_)) {
                source.body()
            } else {
                &[]
            },
            |_| false,
        );
        Context {
            imported: &source.imports,
            scan: &scan,
            constant: None,
        }
        .arg_type(value, &[])
    }

    /// Whether `name` is assigned, annotated or a parameter in the module or in a
    /// function of `stack`.
    fn binds(&self, stack: &[Scope], name: &str) -> bool {
        stack
            .iter()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .any(|&scope| self.scan.typed(scope, name).next().is_some())
    }

    /// The class a variable holds in `stack`: the one every assignment, annotation and
    /// parameter of that name gives, in the innermost scope that has one.
    pub fn type_of(&self, stack: &[Scope], name: &str) -> Option<String> {
        let mut kinds = stack
            .iter()
            .rev()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .map(|&scope| self.scan.typed(scope, name).peekable())
            .find_map(|mut typed| typed.peek().is_some().then_some(typed))?;
        let first = kinds.next()??;
        kinds
            .all(|other| other.as_ref() == Some(&first))
            .then_some(first)
    }

    fn class(&self, name: Option<String>) -> Option<String> {
        class_of(name, self.imported)
    }
}

/// The builtin class of a literal other than `None`.
fn literal_type(expr: &Expr) -> Option<&'static str> {
    let number = |number: &ast::ExprNumberLiteral| match number.value {
        Number::Int(_) => Some("int"),
        Number::Float(_) => Some("float"),
        Number::Complex { .. } => None,
    };
    match expr {
        Expr::BooleanLiteral(_) => Some("bool"),
        Expr::NumberLiteral(literal) => number(literal),
        Expr::StringLiteral(_) | Expr::FString(_) => Some("str"),
        Expr::BytesLiteral(_) => Some("bytes"),
        Expr::UnaryOp(unary) if matches!(unary.op, UnaryOp::UAdd | UnaryOp::USub) => {
            match unary.operand.as_ref() {
                Expr::NumberLiteral(literal) => number(literal),
                Expr::BooleanLiteral(_) => Some("int"),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The type of an instance of the class spelled `name`, when it is
/// a known one.
pub fn class_of(name: Option<String>, imported: &Table) -> Option<String> {
    {
        let name = name?;
        let (head, rest) = name.split_once('.').unwrap_or((&name, ""));
        match imported.get(head) {
            None if BUILTINS.contains(&name.as_str()) => Some(name),
            None if SEQUENCES.contains(&name.as_str()) => Some("list".to_string()),
            None => None,
            Some(imported) => {
                let full = if rest.is_empty() {
                    imported.clone()
                } else {
                    format!("{imported}.{rest}")
                };
                IMPORTED.contains(&full.as_str()).then_some(full)
            }
        }
    }
}

/// 1-based line numbers of byte offsets, counting `\n`, `\r\n` and `\r` as `ast` does.
pub struct Lines(Vec<usize>);

impl Lines {
    /// The line starts of `source`.
    pub fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut starts = vec![0];
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => starts.push(i + 1),
                b'\r' if bytes.get(i + 1) == Some(&b'\n') => {
                    starts.push(i + 2);
                    i += 1;
                }
                b'\r' => starts.push(i + 1),
                _ => {}
            }
            i += 1;
        }
        Self(starts)
    }

    /// The line `offset` is on.
    pub fn line(&self, offset: usize) -> usize {
        self.0.partition_point(|&start| start <= offset)
    }

    /// The offset of the start of the 1-based `line`, or `len` for line 0 or past the last one.
    pub fn start(&self, line: usize, len: usize) -> usize {
        self.0.get(line.wrapping_sub(1)).copied().unwrap_or(len)
    }
}

/// `a.b.c` for a dotted name, else `None`.
pub fn dotted(expr: &Expr) -> Option<String> {
    let mut parts = Vec::new();
    let mut node = expr;
    while let Expr::Attribute(attr) = node {
        parts.push(attr.attr.as_str());
        node = &attr.value;
    }
    let Expr::Name(name) = node else {
        return None;
    };
    parts.push(name.id.as_str());
    parts.reverse();
    Some(parts.join("."))
}

/// `name` with its first part replaced by what it was imported as.
pub fn qualified(name: Option<String>, imported: &Table) -> Option<String> {
    let name = name?;
    let (head, rest) = name.split_once('.').unwrap_or((&name, ""));
    let found = imported.get(head)?;
    Some(if rest.is_empty() {
        found.clone()
    } else {
        format!("{found}.{rest}")
    })
}

/// The dotted name an annotation or a called expression spells.
fn class_name(expr: Option<&Expr>) -> Option<String> {
    match expr? {
        Expr::StringLiteral(text) => {
            let text = text
                .value
                .to_str()
                .trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c));
            text.split('.')
                .all(|part| {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                        && chars.all(crate::types::generic::is_word)
                })
                .then(|| text.to_string())
        }
        expr => dotted(expr),
    }
}
