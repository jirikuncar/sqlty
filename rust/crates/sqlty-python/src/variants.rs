//! The queries a function builds along its paths, as `sqlty._variants` reads them: the
//! walk forks at `if`, `match` and conditional expressions, drops the paths that return
//! or raise, and evaluates the query on each path.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{
    ExceptHandler, Expr, ExprAttribute, ExprCall, ExprContext, ExprName, ExprSubscript, Operator,
    Parameter, Parameters, Pattern, Stmt, StmtFunctionDef, StmtIf, StmtMatch, StmtReturn,
};
use ruff_text_size::{Ranged, TextRange};

use crate::compose::{self, Declared, Resolver, Run, Stop, Value};
use crate::scopes::{dotted, Node as Scope};
use crate::types::info::Dict;
use crate::types::symbols::Source;
use crate::unparse::unparse;

/// Paths followed at most, whatever the limit of variants.
pub const PATH_LIMIT: usize = 4096;
const DEPTH: usize = 64;
const CALLS: usize = 8;

/// One query a call can run, with the lines of the bindings that select it.
#[derive(Clone, Debug)]
pub struct Variant<'a> {
    /// The SQL.
    pub query: String,
    /// Set when the query is that of one `SQL[R, P]("...")`.
    pub declared: Option<Declared<'a>>,
    /// The lines of the bindings that tell it from the other variants.
    pub lines: Vec<usize>,
}

/// What `query_variants` finds.
#[derive(Debug)]
pub enum Found<'a> {
    /// The queries, in the order the paths reach them.
    Variants(Vec<Variant<'a>>),
    /// A part is not static, or there are too many variants (`reason`).
    Unchecked {
        /// Whether a `psycopg.sql` composition was read.
        composed: bool,
        /// Why, when there are too many variants.
        reason: Option<String>,
    },
}

enum Halt<'a> {
    Dynamic,
    TooMany,
    /// Python computes something the model of `compose` does not.
    Unread,
    /// The walk reached the call, along these paths.
    Reached(Vec<Path<'a>>),
}

impl From<Stop> for Halt<'_> {
    fn from(stop: Stop) -> Self {
        match stop {
            Stop::Dynamic | Stop::Error => Halt::Dynamic,
            Stop::Unread => Halt::Unread,
        }
    }
}

type Walked<'a, T> = Result<T, Halt<'a>>;
type Lines = BTreeSet<usize>;

#[derive(Clone)]
struct Bound<'a> {
    /// `None` for a name bound to something that is not static.
    value: Option<Value<'a>>,
    lines: Lines,
}

/// The static names bound so far on one path, and the conditions it took.
#[derive(Clone, Default)]
struct Path<'a> {
    env: BTreeMap<String, Bound<'a>>,
    conds: Dict<(bool, Rc<HashSet<String>>)>,
}

impl<'a> Path<'a> {
    fn kept(&self, names: &HashSet<String>) -> Dict<(bool, Rc<HashSet<String>>)> {
        let mut kept = Dict::new();
        for (key, cond) in self.conds.iter() {
            if cond.1.is_disjoint(names) {
                kept.insert(key, cond.clone());
            }
        }
        kept
    }

    fn bind(&self, name: &str, bound: Bound<'a>) -> Path<'a> {
        let mut env = self.env.clone();
        env.insert(name.to_string(), bound);
        Path {
            env,
            conds: self.kept(&HashSet::from([name.to_string()])),
        }
    }

    fn assume(&self, key: &str, outcome: bool, names: Rc<HashSet<String>>) -> Path<'a> {
        let mut conds = self.conds.clone();
        conds.insert(key, (outcome, names));
        Path {
            env: self.env.clone(),
            conds,
        }
    }

    fn forget(&self, names: &HashSet<String>) -> Path<'a> {
        if names.is_empty() {
            return self.clone();
        }
        Path {
            env: self.env.clone(),
            conds: self.kept(names),
        }
    }

    fn key(&self) -> String {
        let mut key = String::new();
        for (name, bound) in &self.env {
            key.push_str(&format!("{}:{name}", name.len()));
            match &bound.value {
                None => key.push('?'),
                Some(value) => value.key(&mut key),
            }
            key.push_str(&format!("{:?}", bound.lines));
        }
        key
    }
}

/// A statement of a body: an `elif` stands for the `if` Python nests in the `else`.
#[derive(Clone, Copy)]
enum Node<'a> {
    Stmt(&'a Stmt),
    Elif(&'a StmtIf, usize),
}

#[derive(Clone, Copy)]
enum Body<'a> {
    Stmts(&'a [Stmt]),
    Elif(&'a StmtIf, usize),
}

impl<'a> Body<'a> {
    fn nodes(self) -> Vec<Node<'a>> {
        match self {
            Body::Stmts(stmts) => stmts.iter().map(Node::Stmt).collect(),
            Body::Elif(node, index) => vec![Node::Elif(node, index)],
        }
    }
}

/// What follows clause `index` of `node`: the next `elif` as an `if`, or the `else` body.
fn tail(node: &StmtIf, index: usize) -> Body<'_> {
    match node.elif_else_clauses.get(index) {
        None => Body::Stmts(&[]),
        Some(clause) if clause.test.is_some() => Body::Elif(node, index),
        Some(clause) => Body::Stmts(&clause.body),
    }
}

impl<'a> Node<'a> {
    fn range(self) -> TextRange {
        match self {
            Node::Stmt(stmt) => stmt.range(),
            Node::Elif(node, index) => {
                TextRange::new(node.elif_else_clauses[index].start(), node.end())
            }
        }
    }

    fn contains(self, target: TextRange) -> bool {
        self.range().contains_range(target)
    }

    fn id(self) -> (usize, usize) {
        match self {
            Node::Stmt(stmt) => (std::ptr::from_ref(stmt) as usize, usize::MAX),
            Node::Elif(node, index) => (std::ptr::from_ref(node) as usize, index),
        }
    }

    /// `(test, body, orelse)` of an `if`.
    fn branches(self) -> Option<(&'a Expr, Body<'a>, Body<'a>)> {
        match self {
            Node::Stmt(Stmt::If(node)) => {
                Some((&node.test, Body::Stmts(&node.body), tail(node, 0)))
            }
            Node::Stmt(_) => None,
            Node::Elif(node, index) => {
                let clause = &node.elif_else_clauses[index];
                Some((
                    clause.test.as_ref()?,
                    Body::Stmts(&clause.body),
                    tail(node, index + 1),
                ))
            }
        }
    }
}

/// `_assignments`: the names a plain assignment binds, with the value each takes.
fn assignments(stmt: &Stmt) -> Vec<(&ExprName, &Expr)> {
    let mut found = Vec::new();
    match stmt {
        Stmt::Assign(node) => {
            for target in &node.targets {
                let (targets, values) = match (target, node.value.as_ref()) {
                    (Expr::Name(name), value) => {
                        found.push((name, value));
                        continue;
                    }
                    (Expr::Tuple(targets), Expr::Tuple(values)) => (&targets.elts, &values.elts),
                    (Expr::Tuple(targets), Expr::List(values)) => (&targets.elts, &values.elts),
                    (Expr::List(targets), Expr::Tuple(values)) => (&targets.elts, &values.elts),
                    (Expr::List(targets), Expr::List(values)) => (&targets.elts, &values.elts),
                    _ => continue,
                };
                if targets.len() == values.len() {
                    for (target, value) in targets.iter().zip(values) {
                        if let Expr::Name(name) = target {
                            found.push((name, value));
                        }
                    }
                }
            }
        }
        Stmt::AnnAssign(node) => {
            if let (Expr::Name(name), Some(value)) = (node.target.as_ref(), &node.value) {
                found.push((name, value));
            }
        }
        Stmt::AugAssign(node) => {
            if let (Expr::Name(name), Operator::Add) = (node.target.as_ref(), node.op) {
                found.push((name, &node.value));
            }
        }
        _ => {}
    }
    found
}

/// `_own_nodes`: what the statements hold, without the bodies of nested functions,
/// classes or comprehensions.
#[derive(Default)]
struct Own<'a> {
    /// Assignments, and whether a loop, `try` or `with` holds them.
    assigned: Vec<(&'a Stmt, bool)>,
    /// Every name bound, with the `Name` that binds it, if any.
    stores: Vec<(&'a str, Option<*const ExprName>)>,
    /// Names bound with `def`, `class` or `import`.
    defined: Vec<&'a str>,
    /// Names whose object changes: an attribute or item assigned or deleted, a method
    /// called as a statement, a `:=` inside a comprehension.
    mutated: Vec<&'a str>,
    returns: Vec<(&'a StmtReturn, bool)>,
    exits: bool,
    guarded: usize,
}

/// The name an attribute, item or call chain starts from.
fn base(mut expr: &Expr) -> Option<&str> {
    loop {
        expr = match expr {
            Expr::Name(name) => return Some(name.id.as_str()),
            Expr::Attribute(node) => &node.value,
            Expr::Subscript(node) => &node.value,
            Expr::Call(node) => &node.func,
            Expr::Await(node) => &node.value,
            _ => return None,
        };
    }
}

/// What makes a test differ each time it runs: the names `:=` binds in it, and whether
/// it awaits or yields.
#[derive(Default)]
struct Unstable<'a> {
    bound: Vec<&'a str>,
    suspends: bool,
}

impl<'a> Visitor<'a> for Unstable<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => return,
            Expr::Named(node) => {
                if let Expr::Name(name) = node.target.as_ref() {
                    self.bound.push(name.id.as_str());
                }
            }
            Expr::Await(_) | Expr::Yield(_) | Expr::YieldFrom(_) => self.suspends = true,
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

impl<'a> Own<'a> {
    fn of(nodes: &[Node<'a>]) -> Self {
        let mut own = Own::default();
        for node in nodes {
            match *node {
                Node::Stmt(stmt) => own.visit_stmt(stmt),
                Node::Elif(node, index) => {
                    for clause in &node.elif_else_clauses[index..] {
                        if let Some(test) = &clause.test {
                            own.visit_expr(test);
                        }
                        own.visit_body(&clause.body);
                    }
                }
            }
        }
        own
    }

    fn guard(&mut self, body: &'a [Stmt]) {
        self.guarded += 1;
        self.visit_body(body);
        self.guarded -= 1;
    }

    fn bound_names(&self) -> HashSet<String> {
        self.stores
            .iter()
            .map(|(name, _)| *name)
            .chain(self.defined.iter().copied())
            .map(str::to_string)
            .collect()
    }

    /// The names a condition must not have read to hold after these statements.
    fn changed_names(&self) -> HashSet<String> {
        let mut names = self.bound_names();
        names.extend(self.mutated.iter().map(|name| (*name).to_string()));
        names
    }
}

impl<'a> Visitor<'a> for Own<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => self.defined.push(node.name.as_str()),
            Stmt::ClassDef(node) => self.defined.push(node.name.as_str()),
            Stmt::Import(node) => self.defined.extend(node.names.iter().map(|alias| {
                let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                local.split('.').next().unwrap_or(local)
            })),
            Stmt::ImportFrom(node) => self.defined.extend(node.names.iter().map(|alias| {
                let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                local.split('.').next().unwrap_or(local)
            })),
            Stmt::Global(node) => {
                self.stores
                    .extend(node.names.iter().map(|name| (name.as_str(), None)));
            }
            Stmt::Nonlocal(node) => {
                self.stores
                    .extend(node.names.iter().map(|name| (name.as_str(), None)));
            }
            Stmt::Return(node) => {
                self.returns.push((node, self.guarded > 0));
                self.exits = true;
                visitor::walk_stmt(self, stmt);
            }
            Stmt::Raise(_) | Stmt::Break(_) | Stmt::Continue(_) => {
                self.exits = true;
                visitor::walk_stmt(self, stmt);
            }
            Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_) => {
                self.assigned.push((stmt, self.guarded > 0));
                visitor::walk_stmt(self, stmt);
            }
            Stmt::For(node) => {
                self.visit_expr(&node.target);
                self.visit_expr(&node.iter);
                self.guard(&node.body);
                self.guard(&node.orelse);
            }
            Stmt::While(node) => {
                self.visit_expr(&node.test);
                self.guard(&node.body);
                self.guard(&node.orelse);
            }
            Stmt::With(node) => {
                for item in &node.items {
                    self.visit_expr(&item.context_expr);
                    if let Some(target) = &item.optional_vars {
                        self.visit_expr(target);
                    }
                }
                self.guard(&node.body);
            }
            Stmt::Try(node) => {
                self.guard(&node.body);
                for handler in &node.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    if let Some(kind) = &handler.type_ {
                        self.visit_expr(kind);
                    }
                    if let Some(name) = &handler.name {
                        self.stores.push((name.as_str(), None));
                    }
                    self.guard(&handler.body);
                }
                self.guard(&node.orelse);
                self.guard(&node.finalbody);
            }
            Stmt::Expr(node) => {
                let call = match node.value.as_ref() {
                    Expr::Await(awaited) => awaited.value.as_ref(),
                    value => value,
                };
                if let Expr::Call(call) = call {
                    if let Expr::Attribute(method) = call.func.as_ref() {
                        self.mutated.extend(base(&method.value));
                    }
                }
                visitor::walk_stmt(self, stmt);
            }
            _ => visitor::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Lambda(_) => {}
            Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::Generator(_) => {
                let mut inner = Unstable::default();
                visitor::walk_expr(&mut inner, expr);
                self.mutated.extend(inner.bound);
            }
            Expr::Attribute(ExprAttribute { ctx, value, .. })
            | Expr::Subscript(ExprSubscript { ctx, value, .. })
                if *ctx != ExprContext::Load =>
            {
                self.mutated.extend(base(value));
                visitor::walk_expr(self, expr);
            }
            Expr::Name(name) => {
                if name.ctx != ExprContext::Load {
                    self.stores
                        .push((name.id.as_str(), Some(std::ptr::from_ref(name))));
                }
            }
            _ => visitor::walk_expr(self, expr),
        }
    }

    fn visit_pattern(&mut self, pattern: &'a Pattern) {
        let name = match pattern {
            Pattern::MatchAs(node) => node.name.as_ref(),
            Pattern::MatchStar(node) => node.name.as_ref(),
            Pattern::MatchMapping(node) => node.rest.as_ref(),
            _ => None,
        };
        if let Some(name) = name {
            self.stores.push((name.as_str(), None));
        }
        visitor::walk_pattern(self, pattern);
    }
}

/// `_names`: every name `node` reads or binds, at any depth.
struct Names(HashSet<String>);

impl<'a> Visitor<'a> for Names {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            self.0.insert(name.id.to_string());
        }
        visitor::walk_expr(self, expr);
    }
}

fn names(expr: &Expr) -> HashSet<String> {
    let mut found = Names(HashSet::new());
    found.visit_expr(expr);
    found.0
}

/// The names of the parameters, and of those of the lambdas in their defaults.
fn argument_names(parameters: &Parameters, out: &mut HashSet<String>) {
    struct Lambdas<'o>(&'o mut HashSet<String>);
    impl<'a> Visitor<'a> for Lambdas<'_> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Lambda(node) = expr {
                if let Some(parameters) = &node.parameters {
                    argument_names(parameters, self.0);
                }
            }
            visitor::walk_expr(self, expr);
        }
    }
    for parameter in parameters {
        out.insert(parameter.name().to_string());
        let mut lambdas = Lambdas(out);
        if let Some(annotation) = parameter.annotation() {
            lambdas.visit_expr(annotation);
        }
        if let Some(default) = parameter.default() {
            lambdas.visit_expr(default);
        }
    }
}

/// A test of a branch.
#[derive(Clone)]
struct Condition {
    /// `ast.dump(test)`.
    key: Rc<str>,
    /// The names it reads.
    reads: Rc<HashSet<String>>,
    /// The names it binds with `:=`, when it binds, awaits or yields: such a test may take
    /// either branch each time it runs.
    rebinds: Option<Rc<HashSet<String>>>,
}

/// What a walk needs of a module.
struct Module<'a> {
    scan: compose::Scan<'a>,
    /// The names that spell `sqlty.SQL`.
    marker: HashSet<String>,
    /// The frame reading the module-level constants.
    frame: Rc<Frame<'a>>,
}

/// `_Frame`: a function (or the module) whose bindings a walk follows.
struct Frame<'a> {
    source: &'a Source,
    body: &'a [Stmt],
    params: Dict<&'a Parameter>,
    outer: HashSet<String>,
    local: HashSet<String>,
    shadowed: HashSet<String>,
    relevant: HashSet<String>,
    returns_guarded: bool,
}

/// The scope a frame follows.
#[derive(Clone, Copy)]
enum Follow<'a> {
    Function(&'a StmtFunctionDef),
    Module,
    /// No statements: module-level constants only.
    Constants,
}

impl<'a> Frame<'a> {
    fn new(
        source: &'a Source,
        scope: Follow<'a>,
        roots: &[&'a Expr],
        outer: &[Scope<'a>],
    ) -> Option<Self> {
        let body: &'a [Stmt] = match scope {
            Follow::Function(function) => &function.body,
            Follow::Module => source.body(),
            Follow::Constants => &[],
        };
        let mut params = Dict::new();
        if let Follow::Function(function) = scope {
            for parameter in &function.parameters {
                params.insert(parameter.name().as_str(), parameter.as_parameter());
            }
        }
        let mut enclosing = HashSet::new();
        for scope in outer {
            if let Scope::Function(function) = scope {
                let nodes: Vec<Node<'_>> = function.body.iter().map(Node::Stmt).collect();
                enclosing.extend(Own::of(&nodes).bound_names());
                argument_names(&function.parameters, &mut enclosing);
            }
        }
        let nodes: Vec<Node<'a>> = body.iter().map(Node::Stmt).collect();
        let own = Own::of(&nodes);
        let mut values: HashMap<&str, Vec<&Expr>> = HashMap::new();
        let mut followed: HashSet<*const ExprName> = HashSet::new();
        for (stmt, guarded) in &own.assigned {
            if *guarded {
                continue;
            }
            if let Stmt::AnnAssign(node) = stmt {
                if let (Expr::Name(target), None) = (node.target.as_ref(), &node.value) {
                    followed.insert(std::ptr::from_ref(target));
                }
            }
            for (target, value) in assignments(stmt) {
                followed.insert(std::ptr::from_ref(target));
                values.entry(target.id.as_str()).or_default().push(value);
            }
        }
        let other: HashSet<&str> = own
            .stores
            .iter()
            .filter(|(_, store)| store.is_none_or(|store| !followed.contains(&store)))
            .map(|(name, _)| *name)
            .collect();
        let mut relevant: HashSet<String> = HashSet::new();
        let mut todo: Vec<String> = roots.iter().flat_map(|root| names(root)).collect();
        while let Some(name) = todo.pop() {
            if relevant.contains(&name) {
                continue;
            }
            for value in values.get(name.as_str()).into_iter().flatten() {
                todo.extend(names(value));
            }
            relevant.insert(name);
        }
        if relevant.iter().any(|name| other.contains(name.as_str())) {
            return None;
        }
        Some(Frame {
            source,
            body,
            params,
            outer: enclosing,
            local: own
                .stores
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect(),
            // A function of the module is called by name; one defined in a function is not.
            shadowed: match scope {
                Follow::Function(_) => own.defined.iter().map(|name| (*name).to_string()).collect(),
                _ => HashSet::new(),
            },
            relevant,
            returns_guarded: own.returns.iter().any(|(_, guarded)| *guarded),
        })
    }
}

/// `_literal_members`: the strings of a `Literal[...]` annotation.
fn literal_members(annotation: Option<&Expr>, source: &str) -> Option<Vec<String>> {
    fn members(annotation: &Expr, source: &str) -> Option<Vec<String>> {
        let Expr::Subscript(subscript) = annotation else {
            return None;
        };
        if dotted(&subscript.value)?.rsplit('.').next() != Some("Literal") {
            return None;
        }
        let items = match subscript.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.as_slice(),
            other => std::slice::from_ref(other),
        };
        items
            .iter()
            .map(|item| match item {
                Expr::StringLiteral(literal) => Some(crate::parse::text(literal, source)),
                _ => None,
            })
            .collect()
    }
    match annotation? {
        Expr::StringLiteral(literal) => {
            let text = crate::parse::text(literal, source);
            let parsed = crate::parse::expression(&text)?;
            members(&parsed.syntax().body, &text)
        }
        other => members(other, source),
    }
}

/// A value an expression takes on a path, with its lines and the path it leaves.
type Taken<'a> = (Value<'a>, Lines, Path<'a>);

/// What a name or an expression is, where both are read the same way.
#[derive(Clone, Copy)]
enum Operand<'a, 'n> {
    Expr(&'a Expr),
    Name(&'n str),
}

/// `_Walk`: paths through function bodies, and the values of expressions along them.
struct Walk<'a, 'r> {
    resolve: Option<Resolver<'a, 'r>>,
    composed: bool,
    depth: usize,
    calls: usize,
    touches: HashMap<(usize, usize), bool>,
    bound: HashMap<(usize, usize), Rc<HashSet<String>>>,
    modules: HashMap<*const Source, Rc<Module<'a>>>,
    conditions: HashMap<*const Expr, Condition>,
}

impl<'a, 'r> Walk<'a, 'r> {
    fn module(&mut self, source: &'a Source) -> Rc<Module<'a>> {
        let key = std::ptr::from_ref(source);
        if let Some(found) = self.modules.get(&key) {
            return found.clone();
        }
        let frame = Frame::new(source, Follow::Constants, &[], &[]);
        let module = Rc::new(Module {
            scan: compose::Scan::new(source),
            marker: crate::calls::marker_names(source).1,
            frame: Rc::new(frame.unwrap_or(Frame {
                source,
                body: &[],
                params: Dict::new(),
                outer: HashSet::new(),
                local: HashSet::new(),
                shadowed: HashSet::new(),
                relevant: HashSet::new(),
                returns_guarded: false,
            })),
        });
        self.modules.insert(key, module.clone());
        module
    }

    fn condition(&mut self, test: &'a Expr, source: &Source) -> Condition {
        self.conditions
            .entry(std::ptr::from_ref(test))
            .or_insert_with(|| {
                let mut unstable = Unstable::default();
                unstable.visit_expr(test);
                let rebinds = !unstable.bound.is_empty() || unstable.suspends;
                Condition {
                    key: Rc::from(unparse(test, &source.text)),
                    reads: Rc::new(names(test)),
                    rebinds: rebinds.then(|| {
                        Rc::new(
                            unstable
                                .bound
                                .iter()
                                .map(|name| (*name).to_string())
                                .collect(),
                        )
                    }),
                }
            })
            .clone()
    }

    /// `_assume`: the paths that can take `outcome` of `test`.
    fn assume(
        &mut self,
        paths: &[Path<'a>],
        test: &'a Expr,
        outcome: bool,
        source: &Source,
    ) -> Vec<Path<'a>> {
        let Condition {
            key,
            reads,
            rebinds,
        } = self.condition(test, source);
        if let Some(bound) = rebinds {
            return paths.iter().map(|path| path.forget(&bound)).collect();
        }
        paths
            .iter()
            .filter_map(|path| match path.conds.get(&key) {
                Some((known, _)) => (*known == outcome).then(|| path.clone()),
                None => Some(path.assume(&key, outcome, reads.clone())),
            })
            .collect()
    }

    fn bound_names(&mut self, node: Node<'a>) -> Rc<HashSet<String>> {
        self.bound
            .entry(node.id())
            .or_insert_with(|| Rc::new(Own::of(&[node]).changed_names()))
            .clone()
    }

    /// `_Walk.touches`: whether `node` binds a name the query reads, or may leave its block.
    fn touched(&mut self, frame: &Frame<'a>, node: Node<'a>) -> bool {
        *self.touches.entry(node.id()).or_insert_with(|| {
            let own = Own::of(&[node]);
            own.exits
                || own.assigned.iter().any(|(stmt, _)| {
                    assignments(stmt)
                        .iter()
                        .any(|(target, _)| frame.relevant.contains(target.id.as_str()))
                })
        })
    }

    /// `_Walk.block`: the paths that run through `body` to its end.
    fn block(
        &mut self,
        frame: &Frame<'a>,
        body: Body<'a>,
        mut paths: Vec<Path<'a>>,
        target: Option<TextRange>,
        mut returns: Option<&mut Vec<(Value<'a>, Lines)>>,
        branch: bool,
    ) -> Walked<'a, Vec<Path<'a>>> {
        for node in body.nodes() {
            if let Some(target) = target.filter(|target| node.contains(*target)) {
                return Err(self.enter(frame, node, &paths, target, returns.as_deref_mut()));
            }
            paths = self.statement(frame, node, paths, returns.as_deref_mut(), branch)?;
            if paths.len() > PATH_LIMIT {
                return Err(Halt::TooMany);
            }
        }
        Ok(paths)
    }

    /// `_Walk.enter`: walk into the block of `node` holding `target`.
    fn enter(
        &mut self,
        frame: &Frame<'a>,
        node: Node<'a>,
        paths: &[Path<'a>],
        target: TextRange,
        mut returns: Option<&mut Vec<(Value<'a>, Lines)>>,
    ) -> Halt<'a> {
        let all = || paths.to_vec();
        let mut branch = false;
        let blocks: Vec<(Body<'a>, Vec<Path<'a>>)> =
            if let Some((test, body, orelse)) = node.branches() {
                branch = true;
                vec![
                    (body, self.assume(paths, test, true, frame.source)),
                    (orelse, self.assume(paths, test, false, frame.source)),
                ]
            } else {
                match node {
                    Node::Stmt(Stmt::For(stmt)) => vec![
                        (Body::Stmts(&stmt.body), all()),
                        (Body::Stmts(&stmt.orelse), all()),
                    ],
                    Node::Stmt(Stmt::While(stmt)) => vec![
                        (Body::Stmts(&stmt.body), all()),
                        (Body::Stmts(&stmt.orelse), all()),
                    ],
                    Node::Stmt(Stmt::With(stmt)) => vec![(Body::Stmts(&stmt.body), all())],
                    Node::Stmt(Stmt::Try(stmt)) => {
                        let mut blocks = vec![(Body::Stmts(&stmt.body), all())];
                        for handler in &stmt.handlers {
                            let ExceptHandler::ExceptHandler(handler) = handler;
                            blocks.push((Body::Stmts(&handler.body), all()));
                        }
                        blocks.push((Body::Stmts(&stmt.orelse), all()));
                        blocks.push((Body::Stmts(&stmt.finalbody), all()));
                        blocks
                    }
                    Node::Stmt(Stmt::Match(stmt)) => {
                        branch = true;
                        match self.cases(frame, stmt, paths.to_vec()) {
                            Ok(blocks) => blocks,
                            Err(halt) => return halt,
                        }
                    }
                    _ => Vec::new(),
                }
            };
        for (body, entry) in blocks {
            if body.nodes().iter().any(|inner| inner.contains(target)) {
                if let Err(halt) = self.block(
                    frame,
                    body,
                    entry,
                    Some(target),
                    returns.as_deref_mut(),
                    branch,
                ) {
                    return halt;
                }
            }
        }
        Halt::Reached(paths.to_vec())
    }

    fn statement(
        &mut self,
        frame: &Frame<'a>,
        node: Node<'a>,
        paths: Vec<Path<'a>>,
        mut returns: Option<&mut Vec<(Value<'a>, Lines)>>,
        branch: bool,
    ) -> Walked<'a, Vec<Path<'a>>> {
        let forget = |walk: &mut Self, paths: Vec<Path<'a>>| {
            let bound = walk.bound_names(node);
            paths.iter().map(|path| path.forget(&bound)).collect()
        };
        if let Some((test, body, orelse)) = node.branches() {
            if !self.touched(frame, node) {
                return Ok(forget(self, paths));
            }
            let entry = self.assume(&paths, test, true, frame.source);
            let mut taken = self.block(frame, body, entry, None, returns.as_deref_mut(), true)?;
            let entry = self.assume(&paths, test, false, frame.source);
            taken.extend(self.block(frame, orelse, entry, None, returns, true)?);
            return Ok(dedupe(taken));
        }
        let Node::Stmt(stmt) = node else {
            return Ok(forget(self, paths));
        };
        match stmt {
            Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::AugAssign(_) => {
                self.assign(frame, node, stmt, paths, branch)
            }
            Stmt::Match(matched) => {
                if !self.touched(frame, node) {
                    return Ok(forget(self, paths));
                }
                let mut found = Vec::new();
                for (body, entry) in self.cases(frame, matched, paths)? {
                    found.extend(self.block(
                        frame,
                        body,
                        entry,
                        None,
                        returns.as_deref_mut(),
                        true,
                    )?);
                }
                Ok(dedupe(found))
            }
            Stmt::Return(node) => {
                if let Some(returns) = returns {
                    let line = frame.source.lines.line(node.start().to_usize());
                    for path in &paths {
                        let value = node.value.as_deref().ok_or(Halt::Dynamic)?;
                        for (found, mut lines, _) in
                            self.values(Operand::Expr(value), path, frame)?
                        {
                            lines.insert(line);
                            returns.push((found, lines));
                        }
                    }
                }
                Ok(Vec::new())
            }
            Stmt::Raise(_) | Stmt::Break(_) | Stmt::Continue(_) => Ok(Vec::new()),
            _ => Ok(forget(self, paths)),
        }
    }

    /// `_Walk.cases`: the paths entering each case, then those that match none.
    fn cases(
        &mut self,
        frame: &Frame<'a>,
        stmt: &'a StmtMatch,
        mut paths: Vec<Path<'a>>,
    ) -> Walked<'a, Vec<(Body<'a>, Vec<Path<'a>>)>> {
        let mut blocks = Vec::new();
        for case in &stmt.cases {
            let strings = strings(&case.pattern, &frame.source.text);
            let (Some(strings), None, Expr::Name(subject)) =
                (strings, &case.guard, stmt.subject.as_ref())
            else {
                blocks.push((Body::Stmts(&case.body), paths.clone()));
                let irrefutable = case.guard.is_none()
                    && matches!(&case.pattern, Pattern::MatchAs(found) if found.pattern.is_none());
                if irrefutable {
                    return Ok(blocks);
                }
                continue;
            };
            let mut entry = Vec::new();
            let mut rest = Vec::new();
            for path in paths {
                match self.name(subject.id.as_str(), &path, frame) {
                    Err(Halt::Dynamic) => {
                        entry.push(path.clone());
                        rest.push(path);
                    }
                    Err(halt) => return Err(halt),
                    Ok(found) => {
                        for (value, _, after) in found {
                            if value.as_str().is_some_and(|text| strings.contains(text)) {
                                entry.push(after);
                            } else {
                                rest.push(after);
                            }
                        }
                    }
                }
            }
            blocks.push((Body::Stmts(&case.body), entry));
            paths = rest;
        }
        blocks.push((Body::Stmts(&[]), paths));
        Ok(blocks)
    }

    fn assign(
        &mut self,
        frame: &Frame<'a>,
        node: Node<'a>,
        stmt: &'a Stmt,
        paths: Vec<Path<'a>>,
        branch: bool,
    ) -> Walked<'a, Vec<Path<'a>>> {
        let names = self.bound_names(node);
        let tracked: Vec<(&'a ExprName, &'a Expr)> = assignments(stmt)
            .into_iter()
            .filter(|(target, _)| frame.relevant.contains(target.id.as_str()))
            .collect();
        if tracked.is_empty() {
            return Ok(paths.iter().map(|path| path.forget(&names)).collect());
        }
        let line = frame.source.lines.line(stmt.start().to_usize());
        let augmented = stmt.is_aug_assign_stmt();
        let mut found = Vec::new();
        for path in paths {
            // Every value is read before any name is bound.
            let mut options: Vec<(Dict<Bound<'a>>, Path<'a>)> = vec![(Dict::new(), path)];
            for (target, value) in &tracked {
                let name = target.id.as_str();
                let mut next = Vec::new();
                for (bindings, current) in options {
                    let alternatives = if augmented {
                        self.added(Operand::Name(name), value, &current, frame)
                    } else {
                        self.values(Operand::Expr(value), &current, frame)
                    };
                    match alternatives {
                        Err(Halt::Dynamic) => {
                            let mut bindings = bindings;
                            bindings.insert(
                                name,
                                Bound {
                                    value: None,
                                    lines: Lines::new(),
                                },
                            );
                            next.push((bindings, current));
                        }
                        Err(halt) => return Err(halt),
                        Ok(alternatives) => {
                            for (result, mut lines, after) in alternatives {
                                if branch {
                                    lines.insert(line);
                                }
                                let mut bindings = bindings.clone();
                                bindings.insert(
                                    name,
                                    Bound {
                                        value: Some(result),
                                        lines,
                                    },
                                );
                                next.push((bindings, after));
                            }
                        }
                    }
                }
                options = next;
            }
            for (bindings, current) in options {
                let unbound: HashSet<String> = names
                    .iter()
                    .filter(|name| bindings.get(name).is_none())
                    .cloned()
                    .collect();
                let mut current = current.forget(&unbound);
                for (name, binding) in bindings.iter() {
                    current = current.bind(name, binding.clone());
                }
                found.push(current);
            }
        }
        Ok(dedupe(found))
    }

    /// `_Walk.values`: the values `node` takes on `path`; none when the path cannot get there.
    fn values(
        &mut self,
        node: Operand<'a, '_>,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        self.depth += 1;
        let found = if self.depth > DEPTH {
            Err(Halt::Dynamic)
        } else {
            match node {
                Operand::Name(name) => self.name(name, path, frame),
                Operand::Expr(expr) => self.evaluate(expr, path, frame),
            }
        };
        self.depth -= 1;
        found
    }

    /// The values of `left + right`, an `ast.BinOp` of the two.
    fn added(
        &mut self,
        left: Operand<'a, '_>,
        right: &'a Expr,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        self.depth += 1;
        let found = if self.depth > DEPTH {
            Err(Halt::Dynamic)
        } else {
            self.sum(left, right, path, frame)
        };
        self.depth -= 1;
        found
    }

    fn sum(
        &mut self,
        left: Operand<'a, '_>,
        right: &'a Expr,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        let mut found = Vec::new();
        for (a, a_lines, middle) in self.values(left, path, frame)? {
            for (b, b_lines, end) in self.values(Operand::Expr(right), &middle, frame)? {
                let mut lines = a_lines.clone();
                lines.extend(b_lines);
                found.push((a.clone().plus(b)?, lines, end));
            }
        }
        Ok(found)
    }

    fn single(
        &mut self,
        node: &'a Expr,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Value<'a>> {
        let mut found = self.values(Operand::Expr(node), path, frame)?;
        match (found.pop(), found.is_empty()) {
            (Some((value, _, _)), true) => Ok(value),
            _ => Err(Halt::Dynamic),
        }
    }

    fn evaluate(
        &mut self,
        node: &'a Expr,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        let text = &*frame.source.text;
        match node {
            Expr::StringLiteral(literal) => Ok(vec![(
                Value::Str(crate::parse::text(literal, text)),
                Lines::new(),
                path.clone(),
            )]),
            Expr::Name(name) => self.name(name.id.as_str(), path, frame),
            Expr::BinOp(node) if node.op == Operator::Add => {
                self.sum(Operand::Expr(&node.left), &node.right, path, frame)
            }
            Expr::If(node) => {
                let mut found = Vec::new();
                for (branch, outcome) in [(&node.body, true), (&node.orelse, false)] {
                    let line = frame.source.lines.line(branch.start().to_usize());
                    let entry = std::slice::from_ref(path);
                    for taken in self.assume(entry, &node.test, outcome, frame.source) {
                        for (value, mut lines, end) in
                            self.values(Operand::Expr(branch), &taken, frame)?
                        {
                            lines.insert(line);
                            found.push((value, lines, end));
                        }
                    }
                }
                Ok(found)
            }
            Expr::Subscript(node) if !node.slice.is_slice_expr() => {
                let mut found = Vec::new();
                for (whole, w_lines, middle) in
                    self.values(Operand::Expr(&node.value), path, frame)?
                {
                    let Value::Dict(items) = whole else {
                        return Err(Halt::Dynamic);
                    };
                    for (index, k_lines, end) in
                        self.values(Operand::Expr(&node.slice), &middle, frame)?
                    {
                        // Python hashes the key: only a string is one.
                        let key = index.as_str().ok_or(Halt::Unread)?;
                        if let Some((_, value)) = items.iter().find(|(known, _)| known == key) {
                            let mut lines = w_lines.clone();
                            lines.extend(k_lines);
                            found.push((value.clone(), lines, end));
                        }
                    }
                }
                Ok(found)
            }
            Expr::Dict(node) if node.items.iter().all(|item| item.key.is_some()) => {
                let mut built: Vec<(String, Value<'a>)> = Vec::new();
                for item in &node.items {
                    let value = self.single(&item.value, path, frame)?;
                    let Some(key) = &item.key else {
                        return Err(Halt::Dynamic);
                    };
                    let key = self.single(key, path, frame)?;
                    let key = key.as_str().ok_or(Halt::Unread)?.to_string();
                    match built.iter_mut().find(|(known, _)| *known == key) {
                        Some(slot) => slot.1 = value,
                        None => built.push((key, value)),
                    }
                }
                Ok(vec![(Value::Dict(built), Lines::new(), path.clone())])
            }
            Expr::Attribute(_) => self.imported(node, path, frame),
            Expr::Call(call) => {
                if let Some(declared) = self.declared(call, frame) {
                    return Ok(vec![(
                        Value::Declared(declared),
                        Lines::new(),
                        path.clone(),
                    )]);
                }
                self.call(node, call, path, frame)
            }
            _ => Err(Halt::Dynamic),
        }
    }

    /// The query of a `SQL[R]("...")` or `SQL[R, P]("...")` call.
    fn declared(&mut self, call: &'a ExprCall, frame: &Frame<'a>) -> Option<Declared<'a>> {
        let [Expr::StringLiteral(query)] = &*call.arguments.args else {
            return None;
        };
        let (callee, subscript) = match call.func.as_ref() {
            Expr::Subscript(subscript) => (subscript.value.as_ref(), Some(subscript)),
            other => (other, None),
        };
        if !self.module(frame.source).marker.contains(&dotted(callee)?) {
            return None;
        }
        let params = subscript.and_then(|subscript| match subscript.slice.as_ref() {
            Expr::Tuple(tuple) if tuple.elts.len() == 2 => tuple.elts.get(1),
            _ => None,
        });
        Some(Declared {
            query: crate::parse::text(query, &frame.source.text),
            params,
            source: frame.source,
        })
    }

    fn name(
        &mut self,
        name: &str,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        let param = frame.params.get(name).copied();
        if frame.local.contains(name) || param.is_some() {
            if let Some(bound) = path.env.get(name) {
                let value = bound.value.clone().ok_or(Halt::Dynamic)?;
                return Ok(vec![(value, bound.lines.clone(), path.clone())]);
            }
            let Some(param) = param else {
                return Ok(Vec::new());
            };
            let members = literal_members(param.annotation.as_deref(), &frame.source.text)
                .ok_or(Halt::Dynamic)?;
            let line = Lines::from([frame.source.lines.line(param.name().start().to_usize())]);
            return Ok(members
                .into_iter()
                .map(|member| {
                    let bound = Bound {
                        value: Some(Value::Str(member.clone())),
                        lines: line.clone(),
                    };
                    (Value::Str(member), line.clone(), path.bind(name, bound))
                })
                .collect());
        }
        if frame.outer.contains(name) || frame.shadowed.contains(name) {
            return Err(Halt::Dynamic);
        }
        let module = self.module(frame.source);
        if let Some(value) = compose::constant(frame.source, name) {
            let constants = module.frame.clone();
            return self.values(Operand::Expr(value), path, &constants);
        }
        self.resolved(name, path, frame)
    }

    /// `_Walk.imported`: the value of a module-level name, of this module or one it imports.
    fn imported(
        &mut self,
        node: &'a Expr,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        let dotted = dotted(node).ok_or(Halt::Dynamic)?;
        self.resolved(&dotted, path, frame)
    }

    fn resolved(
        &mut self,
        dotted: &str,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        let resolve = self.resolve.ok_or(Halt::Dynamic)?;
        let (source, value) = resolve(frame.source, dotted).ok_or(Halt::Dynamic)?;
        let constants = self.module(source).frame.clone();
        self.values(Operand::Expr(value), path, &constants)
    }

    fn call(
        &mut self,
        whole: &'a Expr,
        node: &'a ExprCall,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        if let Expr::Name(func) = node.func.as_ref() {
            let name = func.id.as_str();
            let hidden = frame.local.contains(name)
                || frame.params.get(name).is_some()
                || frame.outer.contains(name)
                || frame.shadowed.contains(name);
            if !hidden {
                let mut defined = frame.source.body().iter().filter_map(|stmt| match stmt {
                    Stmt::FunctionDef(function) if function.name.as_str() == name => Some(function),
                    _ => None,
                });
                if let (Some(function), None) = (defined.next(), defined.next()) {
                    if function.decorator_list.is_empty() {
                        return self.returns(function, node, path, frame);
                    }
                }
            }
        }
        if !frame.source.text.contains("psycopg") {
            return Err(Halt::Dynamic);
        }
        let mut read: Vec<String> = {
            let mut found = Names(HashSet::new());
            found.visit_expr(&node.func);
            found.visit_arguments(&node.arguments);
            found
                .0
                .into_iter()
                .filter(|name| frame.params.get(name).is_some())
                .collect()
        };
        read.sort();
        let mut paths = vec![path.clone()];
        for param in read {
            let mut next = Vec::new();
            for path in &paths {
                next.extend(
                    self.name(&param, path, frame)?
                        .into_iter()
                        .map(|taken| taken.2),
                );
            }
            paths = next;
        }
        let mut found = Vec::new();
        for path in paths {
            found.push(self.compose(whole, path, frame)?);
        }
        Ok(found)
    }

    fn compose(
        &mut self,
        node: &'a Expr,
        path: Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Taken<'a>> {
        let module = self.module(frame.source);
        let resolve = self.resolve;
        let read = RefCell::new(Lines::new());
        let failed: Cell<Option<Halt<'a>>> = Cell::new(None);
        let walk = RefCell::new(&mut *self);
        let lookup = |name: &ExprName| -> Result<Value<'a>, Stop> {
            let found = walk.borrow_mut().name(name.id.as_str(), &path, frame);
            let mut found = match found {
                Ok(found) => found,
                Err(halt) => {
                    failed.set(Some(halt));
                    return Err(Stop::Dynamic);
                }
            };
            match (found.pop(), found.is_empty()) {
                (Some((value, lines, _)), true) => {
                    read.borrow_mut().extend(lines);
                    Ok(value)
                }
                _ => {
                    failed.set(Some(Halt::Dynamic));
                    Err(Stop::Dynamic)
                }
            }
        };
        let mut run = Run::new(&module.scan, resolve, Some(&lookup));
        let value = run.value(node);
        let seen = run.seen;
        if let Some(halt) = failed.take() {
            return Err(halt);
        }
        match value {
            Ok(value) => {
                self.composed = true;
                Ok((value, read.into_inner(), path))
            }
            Err(Stop::Unread) => Err(Halt::Unread),
            Err(_) => {
                self.composed = self.composed || seen;
                Err(Halt::Dynamic)
            }
        }
    }

    /// `_Walk.returns`: what `function` returns when `node` calls it.
    fn returns(
        &mut self,
        function: &'a StmtFunctionDef,
        node: &'a ExprCall,
        path: &Path<'a>,
        frame: &Frame<'a>,
    ) -> Walked<'a, Vec<Taken<'a>>> {
        if self.calls >= CALLS || node.arguments.args.iter().any(Expr::is_starred_expr) {
            return Err(Halt::Dynamic);
        }
        let nodes: Vec<Node<'a>> = function.body.iter().map(Node::Stmt).collect();
        let roots: Vec<&'a Expr> = Own::of(&nodes)
            .returns
            .iter()
            .filter_map(|(found, _)| found.value.as_deref())
            .collect();
        let callee = Frame::new(frame.source, Follow::Function(function), &roots, &[])
            .ok_or(Halt::Dynamic)?;
        if callee.returns_guarded {
            return Err(Halt::Dynamic);
        }
        let parameters = &function.parameters;
        let positional = || parameters.posonlyargs.iter().chain(&parameters.args);
        let mut defaults: HashMap<&str, &'a Expr> = HashMap::new();
        for parameter in positional().chain(&parameters.kwonlyargs) {
            if let Some(default) = parameter.default.as_deref() {
                defaults.insert(parameter.parameter.name.as_str(), default);
            }
        }
        let mut passed: HashMap<&str, &'a Expr> = HashMap::new();
        for (parameter, value) in positional().zip(&*node.arguments.args) {
            passed.insert(parameter.parameter.name.as_str(), value);
        }
        for keyword in &*node.arguments.keywords {
            if let Some(name) = &keyword.arg {
                passed.insert(name.as_str(), &keyword.value);
            }
        }
        let mut read: Vec<&str> = callee
            .params
            .iter()
            .map(|(name, _)| name)
            .filter(|name| callee.relevant.contains(*name))
            .collect();
        read.sort_unstable();
        // Each entry binds the arguments of one call, with their lines and the caller's path.
        let mut entries: Vec<(Path<'a>, Lines, Path<'a>)> =
            vec![(Path::default(), Lines::new(), path.clone())];
        let constants = self.module(frame.source).frame.clone();
        for name in read {
            let annotation = callee
                .params
                .get(name)
                .and_then(|param| param.annotation.as_deref());
            let literal = literal_members(annotation, &frame.source.text).is_some();
            if !passed.contains_key(name) && !defaults.contains_key(name) {
                if literal {
                    continue;
                }
                return Err(Halt::Dynamic);
            }
            let mut forked = Vec::new();
            for (entry, lines, caller) in entries {
                let found = match passed.get(name) {
                    Some(value) => self.values(Operand::Expr(value), &caller, frame),
                    None => self
                        .values(Operand::Expr(defaults[name]), &Path::default(), &constants)
                        .map(|found| {
                            found
                                .into_iter()
                                .map(|(value, lines, _)| (value, lines, caller.clone()))
                                .collect()
                        }),
                };
                match found {
                    Err(Halt::Dynamic) => {
                        let unknown = Bound {
                            value: None,
                            lines: Lines::new(),
                        };
                        let entry = if literal {
                            entry
                        } else {
                            entry.bind(name, unknown)
                        };
                        forked.push((entry, lines, caller));
                    }
                    Err(halt) => return Err(halt),
                    Ok(found) => {
                        for (value, v_lines, after) in found {
                            let bound = Bound {
                                value: Some(value),
                                lines: Lines::new(),
                            };
                            let mut lines = lines.clone();
                            lines.extend(v_lines);
                            forked.push((entry.bind(name, bound), lines, after));
                        }
                    }
                }
            }
            entries = forked;
        }
        let mut results = Vec::new();
        self.calls += 1;
        let mut walked = Ok(());
        for (entry, lines, caller) in entries {
            let mut collected = Vec::new();
            let body = Body::Stmts(&function.body);
            match self.block(
                &callee,
                body,
                vec![entry],
                None,
                Some(&mut collected),
                false,
            ) {
                Ok(paths) if paths.is_empty() => {}
                Ok(_) => walked = Err(Halt::Dynamic),
                Err(halt) => walked = Err(halt),
            }
            if walked.is_err() {
                break;
            }
            for (value, v_lines) in collected {
                let mut lines = lines.clone();
                lines.extend(v_lines);
                results.push((value, lines, caller.clone()));
            }
        }
        self.calls -= 1;
        walked.map(|()| results)
    }
}

/// `_dedupe`: one path per set of bindings, keeping the conditions every merged path has.
fn dedupe(paths: Vec<Path<'_>>) -> Vec<Path<'_>> {
    let mut order: Vec<String> = Vec::new();
    let mut found: HashMap<String, Path<'_>> = HashMap::new();
    for path in paths {
        let key = path.key();
        match found.get_mut(&key) {
            None => {
                order.push(key.clone());
                found.insert(key, path);
            }
            Some(kept) => {
                let mut conds = Dict::new();
                for (name, cond) in kept.conds.iter() {
                    let same = path
                        .conds
                        .get(name)
                        .is_some_and(|other| other.0 == cond.0 && other.1 == cond.1);
                    if same {
                        conds.insert(name, cond.clone());
                    }
                }
                kept.conds = conds;
            }
        }
    }
    order
        .into_iter()
        .filter_map(|key| found.remove(&key))
        .collect()
}

/// `_strings`: the strings a pattern of string literals matches.
fn strings(pattern: &Pattern, source: &str) -> Option<HashSet<String>> {
    match pattern {
        Pattern::MatchValue(node) => match node.value.as_ref() {
            Expr::StringLiteral(literal) => {
                Some(HashSet::from([crate::parse::text(literal, source)]))
            }
            _ => None,
        },
        Pattern::MatchOr(node) => {
            let mut found = HashSet::new();
            for alternative in &node.patterns {
                found.extend(strings(alternative, source)?);
            }
            Some(found)
        }
        _ => None,
    }
}

/// `query_variants`: the queries `query`, an argument of the call at `call`, takes on the
/// paths reaching it; `stack` are the functions and classes around the call, outermost
/// first. `None` where Python computes something the model of [`compose`] does not.
pub fn query_variants<'a>(
    source: &'a Source,
    stack: &[Scope<'a>],
    call: TextRange,
    query: &'a Expr,
    limit: usize,
    resolve: Option<Resolver<'a, '_>>,
) -> Option<Found<'a>> {
    let mut walk = Walk {
        resolve,
        composed: false,
        depth: 0,
        calls: 0,
        touches: HashMap::new(),
        bound: HashMap::new(),
        modules: HashMap::new(),
        conditions: HashMap::new(),
    };
    let (scope, outer) = match stack.split_last() {
        None | Some((Scope::Module, _)) => (Follow::Module, &[][..]),
        Some((Scope::Function(function), outer)) => (Follow::Function(function), outer),
        Some((Scope::Class(_), _)) => {
            return Some(Found::Unchecked {
                composed: false,
                reason: None,
            })
        }
    };
    let unchecked = |walk: &Walk<'_, '_>, reason: Option<String>| {
        Some(Found::Unchecked {
            composed: walk.composed,
            reason,
        })
    };
    let Some(frame) = Frame::new(source, scope, &[query], outer) else {
        return unchecked(&walk, None);
    };
    let mut found: Vec<(String, Option<Declared<'a>>, Lines)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let walked: Walked<'a, ()> = (|| {
        let body = Body::Stmts(frame.body);
        let paths = match walk.block(&frame, body, vec![Path::default()], Some(call), None, false) {
            Err(Halt::Reached(paths)) => paths,
            Err(halt) => return Err(halt),
            Ok(_) => Vec::new(),
        };
        for path in paths {
            for (value, lines, _) in walk.values(Operand::Expr(query), &path, &frame)? {
                let (text, declared) = match value {
                    Value::Declared(declared) => (declared.query.clone(), Some(declared)),
                    Value::Str(text) => (text, None),
                    other => (other.as_string()?, None),
                };
                if seen.insert(text.clone()) {
                    found.push((text, declared, lines));
                }
            }
        }
        Ok(())
    })();
    match walked {
        Ok(()) => {}
        Err(Halt::Unread) => return None,
        Err(Halt::TooMany) => {
            let reason =
                format!("over {PATH_LIMIT} variants, more than max-query-variants ({limit})");
            return unchecked(&walk, Some(reason));
        }
        Err(Halt::Dynamic | Halt::Reached(_)) => return unchecked(&walk, None),
    }
    if found.is_empty() {
        return unchecked(&walk, None);
    }
    if found.len() > limit {
        let reason = format!(
            "{} variants, more than max-query-variants ({limit})",
            found.len()
        );
        return unchecked(&walk, Some(reason));
    }
    let shared: Lines = found
        .iter()
        .map(|(_, _, lines)| lines.clone())
        .reduce(|shared, lines| shared.intersection(&lines).copied().collect())
        .unwrap_or_default();
    Some(Found::Variants(
        found
            .into_iter()
            .map(|(query, declared, lines)| {
                let own: Vec<usize> = lines.difference(&shared).copied().collect();
                Variant {
                    query,
                    declared,
                    lines: if own.is_empty() {
                        lines.into_iter().collect()
                    } else {
                        own
                    },
                }
            })
            .collect(),
    ))
}
