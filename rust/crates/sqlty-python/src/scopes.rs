//! The scopes of a module, the classes its names and `self` attributes hold, and the
//! argument types read from them.

use std::collections::{HashMap, HashSet};

use crate::frozen::Table;
use crate::types::symbols::Source;
use ruff_python_ast::visitor::{walk_except_handler, walk_expr, walk_pattern, walk_stmt, Visitor};
use ruff_python_ast::{
    self as ast, Expr, ExprCall, ExprTString, Number, Operator, Stmt, StmtClassDef,
    StmtFunctionDef, UnaryOp,
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
/// every call and t-string with its innermost scope, and every string literal.
pub struct Scan<'a> {
    /// The scopes, by id.
    pub scopes: Vec<Node<'a>>,
    /// The scope around each scope; the module is its own.
    pub parents: Vec<Scope>,
    stack: Vec<Scope>,
    /// Every binding, as the walk meets it, with its scope and name.
    written: Vec<(Scope, &'a str, Binding<'a>)>,
    /// Every binding: those of a name of a scope lie together, by `site`.
    bound: Vec<Binding<'a>>,
    /// Each name of each scope and where its bindings are in `bound`: the names of a
    /// scope lie together, from where `starts` says, in the order the scope binds them.
    names: Vec<(&'a str, std::ops::Range<usize>)>,
    starts: Vec<usize>,
    /// Where each name is in `names`, for the scopes that bind [`FEW`] names or more:
    /// the names of the others, which most scopes are, are read one after the other.
    index: HashMap<(Scope, &'a str), usize>,
    /// Every `self.name` a method binds, by its class, with the method.
    attributes: HashMap<(Scope, &'a str), Vec<Written<'a>>>,
    /// Where a name is one of its own in a scope: in a comprehension or a lambda that
    /// binds it, and in the header of a class or a function that has it as a type
    /// parameter.
    shadows: HashMap<(Scope, &'a str), Vec<TextRange>>,
    /// The lambdas and generator expressions of a scope: code that runs when it is
    /// called or iterated, not where it is written.
    deferred: HashMap<Scope, Vec<TextRange>>,
    /// The code in the body of a class that runs in a scope of its own, where the names
    /// of the class are not seen: the body of a lambda, and a comprehension or generator
    /// expression but for its first iterable.
    sealed: HashMap<Scope, Vec<TextRange>>,
    /// The bodies of the lambdas among them: they run when called, not where written.
    called: HashMap<Scope, Vec<TextRange>>,
    /// The attributes the methods of each class bind, in order: reading those of one
    /// class does not read those of every other.
    fields: HashMap<Scope, Vec<&'a str>>,
    /// The names a function declares `global` or `nonlocal`.
    foreign: HashSet<(Scope, &'a str)>,
    /// Every call, in visit order, with its innermost scope.
    pub calls: Vec<(&'a ExprCall, Scope)>,
    /// Every t-string, with its innermost scope.
    pub templates: Vec<(&'a ExprTString, Scope)>,
    /// Every string literal, f-string and t-string, and whether it is an operand of a `+`.
    pub strings: Vec<(&'a Expr, bool)>,
    /// The scopes around each scope's contents, innermost last, classes included.
    paths: Vec<Vec<Scope>>,
    /// The scope of each class, by where its name is written.
    classes: HashMap<u32, Scope>,
    operands: HashSet<usize>,
}

/// The names a scope binds before they are looked up in a table: fewer are compared one
/// after the other, where they lie together.
const FEW: usize = 8;

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

/// What a method assigns to an attribute of its first parameter: the value, and the
/// method by its scope and its definition.
pub type Written<'a> = (Origin<'a>, Scope, &'a StmtFunctionDef);

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
    /// The walks [`Scan::new`] has made on this thread.
    pub(crate) static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
    /// Walks `body`.
    pub fn new(body: &'a [Stmt]) -> Self {
        #[cfg(test)]
        WALKS.with(|count| count.set(count.get() + 1));
        let mut walker = Walker {
            scan: Self {
                scopes: vec![Node::Module],
                parents: vec![0],
                stack: Vec::new(),
                written: Vec::new(),
                bound: Vec::new(),
                names: Vec::new(),
                starts: Vec::new(),
                index: HashMap::new(),
                attributes: HashMap::new(),
                shadows: HashMap::new(),
                deferred: HashMap::new(),
                sealed: HashMap::new(),
                called: HashMap::new(),
                fields: HashMap::new(),
                foreign: HashSet::new(),
                calls: Vec::new(),
                templates: Vec::new(),
                strings: Vec::new(),
                paths: vec![Vec::new()],
                classes: HashMap::new(),
                operands: HashSet::new(),
            },
        };
        walker.visit_body(body);
        let mut scan = walker.scan;
        scan.order();
        for &(scope, name) in scan.attributes.keys() {
            scan.fields.entry(scope).or_default().push(name);
        }
        scan.fields
            .values_mut()
            .for_each(|names| names.sort_unstable());
        scan.shadows.values_mut().for_each(outermost);
        scan.deferred.values_mut().for_each(outermost);
        scan.sealed.values_mut().for_each(outermost);
        scan.called.values_mut().for_each(outermost);
        scan
    }

    /// The scope of the class whose name is written at offset `site`.
    pub fn class_at(&self, site: u32) -> Option<Scope> {
        self.classes.get(&site).copied()
    }

    /// Whether `scope` is a class.
    pub fn is_class(&self, scope: Scope) -> bool {
        self.class(scope).is_some()
    }

    /// The class `scope` is the body of.
    pub fn class(&self, scope: Scope) -> Option<&'a StmtClassDef> {
        match self.scopes.get(scope)? {
            Node::Class(class) => Some(class),
            _ => None,
        }
    }

    /// Puts the bindings the walk met together: by scope, then by name, then by `site`.
    /// The scopes are told apart by counting and the names by one lookup for each
    /// binding, among the few names of the scope or in `index`, so that a module costs
    /// what it binds: nothing is sorted by name.
    fn order(&mut self) {
        let written = std::mem::take(&mut self.written);
        let scopes = self.scopes.len();
        let mut next = vec![0; scopes + 1];
        for (scope, _, _) in &written {
            next[scope + 1] += 1;
        }
        for scope in 0..scopes {
            next[scope + 1] += next[scope];
        }
        // The bindings by scope, each scope's as the walk met them.
        let mut placed: Vec<usize> = vec![0; written.len()];
        for (at, (scope, _, _)) in written.iter().enumerate() {
            placed[next[*scope]] = at;
            next[*scope] += 1;
        }
        // The name each one binds, among the names of all scopes, and how many do.
        let mut index = HashMap::new();
        let mut names: Vec<(&'a str, std::ops::Range<usize>)> = Vec::new();
        let mut starts = vec![0; scopes + 1];
        let mut group = vec![0; written.len()];
        // The scope being read and where its names start.
        let (mut reading, mut first) = (0, 0);
        for (slot, &at) in placed.iter().enumerate() {
            let (scope, name, _) = written[at];
            if scope != reading {
                (reading, first) = (scope, names.len());
            }
            let few = names.len() - first < FEW;
            let known = match few {
                true => names[first..]
                    .iter()
                    .position(|(known, _)| *known == name)
                    .map(|found| first + found),
                false => index.get(&(scope, name)).copied(),
            };
            let known = known.unwrap_or_else(|| {
                names.push((name, 0..0));
                starts[scope + 1] += 1;
                let new = names.len() - 1;
                if new - first + 1 == FEW {
                    // No longer few: each binding left may bind another name.
                    index.reserve(FEW + next[scope] - slot);
                    let of_scope = names[first..].iter().enumerate();
                    index.extend(of_scope.map(|(nth, (name, _))| ((scope, *name), first + nth)));
                } else if !few {
                    index.insert((scope, name), new);
                }
                new
            });
            names[known].1.end += 1;
            group[slot] = known;
        }
        for scope in 0..scopes {
            starts[scope + 1] += starts[scope];
        }
        let mut start = 0;
        for (_, range) in &mut names {
            let count = range.end;
            *range = start..start;
            start += count;
        }
        let nothing = Binding {
            origin: Origin::Unknown,
            site: 0,
            effect: 0,
        };
        let mut bound = vec![nothing; written.len()];
        for (slot, &at) in placed.iter().enumerate() {
            let range = &mut names[group[slot]].1;
            bound[range.end] = written[at].2;
            range.end += 1;
        }
        for (_, range) in &names {
            let of_name = &mut bound[range.clone()];
            if !of_name.is_sorted_by_key(|binding| binding.site) {
                of_name.sort_by_key(|binding| binding.site);
            }
        }
        (self.bound, self.names, self.starts, self.index) = (bound, names, starts, index);
    }

    /// Every binding of `name` in `scope`, by `site`; `None` when the scope does not bind it.
    pub fn bindings(&self, scope: Scope, name: &str) -> Option<&[Binding<'a>]> {
        let (start, end) = (self.starts[scope], self.starts[scope + 1]);
        let known = match end - start < FEW {
            true => {
                let mut of_scope = self.names[start..end].iter();
                start + of_scope.position(|(known, _)| *known == name)?
            }
            false => *self.index.get(&(scope, name))?,
        };
        Some(&self.bound[self.names[known].1.clone()])
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
        let names = &self.names[self.starts[scope]..self.starts[scope + 1]];
        names.iter().map(|(name, range)| {
            read();
            (*name, &self.bound[range.clone()])
        })
    }

    /// The attributes the methods of the class `scope` bind on their first parameter, each
    /// with the method that does.
    pub fn attributes_of(&self, scope: Scope) -> impl Iterator<Item = (&'a str, &[Written<'a>])> {
        let names = self.fields.get(&scope).map_or(&[][..], Vec::as_slice);
        names.iter().filter_map(move |&name| {
            read();
            let bound = self.attributes.get(&(scope, name))?;
            Some((name, bound.as_slice()))
        })
    }

    /// Whether `at` is inside a comprehension or a lambda of `scope` that binds `name`,
    /// or in the header of a class or a function whose type parameter it is.
    pub fn shadowed(&self, scope: Scope, name: &str, at: u32) -> bool {
        self.shadows
            .get(&(scope, name))
            .is_some_and(|ranges| around(ranges, at).is_some())
    }

    /// Whether a comprehension, a lambda or the type parameters of a class or function
    /// header of `scope` bind `name` anywhere.
    pub fn shadows(&self, scope: Scope, name: &str) -> bool {
        self.shadows.contains_key(&(scope, name))
    }

    /// Where the outermost lambda or generator expression of `scope` around `at` starts.
    /// It runs later: a binding the scope makes from there on may be in effect by then,
    /// as for a nested `def`.
    pub fn deferred(&self, scope: Scope, at: u32) -> Option<u32> {
        around(self.deferred.get(&scope)?, at)
    }

    /// Whether `at` is in code of the class `scope` that does not see its names: the
    /// body of a lambda, or a comprehension past its first iterable.
    pub fn sealed(&self, scope: Scope, at: u32) -> bool {
        self.sealed
            .get(&scope)
            .is_some_and(|ranges| around(ranges, at).is_some())
    }

    /// Whether `at` is in the body of a lambda written in the body of the class `scope`:
    /// it runs when called, after the statements around the class have run.
    pub fn called_later(&self, scope: Scope, at: u32) -> bool {
        self.called
            .get(&scope)
            .is_some_and(|ranges| around(ranges, at).is_some())
    }

    fn scope(&self) -> Scope {
        self.stack.last().copied().unwrap_or(0)
    }

    /// The scopes inside `scope`, outermost first, `scope` last; empty for the module.
    pub fn stack(&self, scope: Scope) -> &[Scope] {
        &self.paths[scope]
    }

    /// Records that `range` of the innermost scope, a class, runs in a scope of its own.
    fn seal(&mut self, range: TextRange) {
        let scope = self.scope();
        if self.is_class(scope) && !range.is_empty() {
            self.sealed.entry(scope).or_default().push(range);
        }
    }

    /// Seals a comprehension `whole` but for the iterable of its first `for`, which is
    /// read where the comprehension is written.
    fn seal_comprehension(&mut self, whole: TextRange, generators: &[ast::Comprehension]) {
        let read = generators.first().map_or(whole, |first| first.iter.range());
        self.seal(TextRange::new(whole.start(), read.start()));
        self.seal(TextRange::new(read.end(), whole.end()));
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
                self.written.push((outer, name, binding(Origin::Unknown)));
            }
            return;
        }
        self.written.push((scope, name, binding(origin)));
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
                            .push((origin, method, function));
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
        if let Node::Class(class) = node {
            self.classes.insert(class.name.start().to_u32(), scope);
        }
        self.scopes.push(node);
        self.parents.push(self.scope());
        let mut path = self.stack.clone();
        path.push(scope);
        self.paths.push(path);
        scope
    }
}

struct Walker<'a> {
    scan: Scan<'a>,
}

/// The name of a type parameter.
pub fn type_parameter(param: &ast::TypeParam) -> &ast::Identifier {
    match param {
        ast::TypeParam::TypeVar(param) => &param.name,
        ast::TypeParam::ParamSpec(param) => &param.name,
        ast::TypeParam::TypeVarTuple(param) => &param.name,
    }
}

impl<'a> Walker<'a> {
    fn string(&mut self, expr: &'a Expr) {
        let operand = self.scan.operands.contains(&expr.start().to_usize());
        self.scan.strings.push((expr, operand));
    }

    /// Reads the type parameters of a `def` or a `class` in the scope around it, where
    /// each is a name of its own from there to `end`, the end of the header.
    fn header(&mut self, type_params: &'a ast::TypeParams, end: TextSize) {
        let header = TextRange::new(type_params.start(), end);
        let around = self.scan.scope();
        for param in type_params {
            let name = type_parameter(param).as_str();
            let hidden = self.scan.shadows.entry((around, name)).or_default();
            hidden.push(header);
        }
        self.visit_type_params(type_params);
    }

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

impl<'a> Visitor<'a> for Walker<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                self.scan.bind_name(
                    node.name.as_str(),
                    Origin::Function(node),
                    node.name.start(),
                    node.end(),
                );
                // The decorators, defaults and annotations of a function are read in the
                // scope around it, where its parameters are not seen.
                for decorator in &node.decorator_list {
                    self.visit_decorator(decorator);
                }
                if let Some(type_params) = node.type_params.as_deref() {
                    let end = node
                        .returns
                        .as_deref()
                        .map_or(node.parameters.end(), Ranged::end);
                    self.header(type_params, end);
                }
                self.visit_parameters(&node.parameters);
                if let Some(returns) = node.returns.as_deref() {
                    self.visit_annotation(returns);
                }
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
                self.visit_body(&node.body);
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
                // The decorators, bases and keywords of a class are read in the scope
                // around it, where the names its body binds are not seen.
                for decorator in &node.decorator_list {
                    self.visit_decorator(decorator);
                }
                if let Some(type_params) = node.type_params.as_deref() {
                    let end = node
                        .arguments
                        .as_deref()
                        .map_or(type_params.end(), Ranged::end);
                    self.header(type_params, end);
                }
                if let Some(arguments) = node.arguments.as_deref() {
                    self.visit_arguments(arguments);
                }
                let scope = self.scan.enter(Node::Class(node));
                self.scan.stack.push(scope);
                self.bind_type_params(node.type_params.as_deref());
                self.visit_body(&node.body);
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
            Expr::Call(call) => {
                let scope = self.scan.scope();
                self.scan.calls.push((call, scope));
            }
            Expr::TString(template) => {
                let scope = self.scan.scope();
                self.scan.templates.push((template, scope));
                self.string(expr);
            }
            Expr::BinOp(op) if op.op == Operator::Add => {
                self.scan.operands.insert(op.left.start().to_usize());
                self.scan.operands.insert(op.right.start().to_usize());
            }
            Expr::StringLiteral(_) | Expr::FString(_) => self.string(expr),
            Expr::Named(named) => self.scan.bind(&named.target, Origin::Unknown, named.end()),
            Expr::Lambda(lambda) => {
                self.scan.seal(lambda.body.range());
                let scope = self.scan.scope();
                if self.scan.is_class(scope) {
                    let bodies = self.scan.called.entry(scope).or_default();
                    bodies.push(lambda.body.range());
                }
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
                self.scan.seal_comprehension(expr.range(), generators);
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
                self.scan.seal_comprehension(expr.range(), generators);
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

/// The class each module-level name of `source` holds, from one walk of the module.
fn module_classes(source: &Source) -> HashMap<String, Option<String>> {
    let scan = Scan::new(source.body());
    let context = Context {
        imported: &source.imports,
        scan: &scan,
        constant: None,
    };
    let class = |name: &str| context.class(context.type_of(&[], name));
    scan.bound_in(0)
        .map(|(name, _)| (name.to_string(), class(name)))
        .collect()
}

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
        if let Expr::Name(name) = value {
            let classes = source.classes.get_or_init(|| module_classes(source));
            return classes.get(name.id.as_str()).cloned().flatten();
        }
        Context {
            imported: &source.imports,
            scan: &Scan::new(&[]),
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
    if let Expr::Name(name) = expr {
        return Some(name.id.to_string());
    }
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use ruff_python_parser::parse_module;

    #[test]
    fn one_walk_collects_calls_templates_strings_and_stacks() {
        let text = "class A:\n    def m(self):\n        f(lambda: g(1), [h(x) for x in y])\n\
                    k = f'a{b}' + 'c'\nt = t'd'\ndef n():\n    return 'e'\n";
        let parsed = parse_module(text).unwrap();
        let scan = Scan::new(&parsed.syntax().body);
        let names: Vec<(String, Scope)> = scan
            .calls
            .iter()
            .map(|(call, scope)| (dotted(&call.func).unwrap(), *scope))
            .collect();
        let expected = [("f", 2), ("g", 2), ("h", 2)].map(|(n, s)| (n.to_string(), s));
        assert_eq!(names, expected);
        assert_eq!(scan.stack(0), &[] as &[Scope]);
        assert_eq!(scan.stack(1), &[1]);
        assert_eq!(scan.stack(2), &[1, 2]);
        assert_eq!(scan.stack(3), &[3]);
        assert_eq!(scan.templates.len(), 1);
        assert_eq!(scan.templates[0].1, 0);
        let strings: Vec<bool> = scan.strings.iter().map(|(_, operand)| *operand).collect();
        assert_eq!(strings, [true, true, false, false]);
        assert!(matches!(scan.strings[2].0, Expr::TString(_)));
        assert!(matches!(scan.strings[3].0, Expr::StringLiteral(_)));
    }
    #[test]
    fn a_scope_of_few_or_many_names_gives_each_its_bindings_in_order() {
        // Below `FEW` names a scope is read name by name, from there on through the
        // table: both give the same answers, on either side of the boundary.
        for count in [1, FEW - 1, FEW, FEW + 1, 5 * FEW] {
            let names: Vec<String> = (0..count).rev().map(|at| format!("n{at}")).collect();
            // Each name twice in the module, in turn, and once more in a function that
            // binds as many names of its own.
            let twice: String = names.iter().map(|name| format!("{name} = 1\n")).collect();
            let inner: String = names
                .iter()
                .map(|name| format!("    {name} = 2\n"))
                .collect();
            let text = format!("{twice}{twice}def f():\n{inner}");
            let parsed = parse_module(&text).unwrap();
            let scan = Scan::new(&parsed.syntax().body);
            for (scope, times) in [(0, 2), (1, 1)] {
                let own = if scope == 0 { count + 1 } else { count };
                // In the order the scope binds them first, not by name.
                let bound: Vec<&str> = scan.bound_in(scope).map(|(name, _)| name).collect();
                assert_eq!(bound.len(), own, "{count} names in scope {scope}");
                assert!(bound.iter().zip(&names).all(|(bound, name)| bound == name));
                for name in &names {
                    let bindings = scan.bindings(scope, name).expect("bound");
                    assert_eq!(bindings.len(), times, "{name} of {count} in {scope}");
                    assert!(bindings.is_sorted_by_key(|binding| binding.site));
                    let written = |binding: &Binding| {
                        let at = binding.site as usize;
                        text[at..].starts_with(&format!("{name} ="))
                    };
                    assert!(bindings.iter().all(written));
                }
                assert!(scan.bindings(scope, "n").is_none());
                assert!(scan.bindings(scope, "missing").is_none());
            }
            assert_eq!(scan.bindings(0, "f").map(<[_]>::len), Some(1));
            assert!(scan.bindings(1, "f").is_none());
        }
    }
}
