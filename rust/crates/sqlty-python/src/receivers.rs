//! The class of a call's receiver, read from declarations the way a type checker reads
//! them: annotations, `x = Cls(...)`, attributes declared on a class or its bases, return
//! annotations, `with ... as`. Nothing is inferred from a function body, nothing is
//! imported or run; what no declaration states is unknown and the call is skipped.
//!
//! [`Facts`] is what one module declares, owned so that the index keeps it per module;
//! [`Resolver`] evaluates the receiver of a call of the checked module against them.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ruff_python_ast::{Expr, Operator, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::calls::{method_kind, Helper, Kind};
use crate::index::{HelperIndex, Targets};
use crate::scopes::{dotted, Node, Origin, Scan, Scope};
use crate::types::info::Dict;
use crate::types::symbols::Source;

/// Modules whose names are read by name, never from their source.
const KNOWN: [&str; 7] = [
    "typing",
    "typing_extensions",
    "contextlib",
    "collections.abc",
    "builtins",
    "abc",
    "sqlty",
];
const TYPING: [&str; 2] = ["typing", "typing_extensions"];
/// `Wrapper[T]` that is `T` to a receiver.
const TRANSPARENT: [&str; 7] = [
    "Optional",
    "ClassVar",
    "Final",
    "Annotated",
    "Required",
    "NotRequired",
    "ReadOnly",
];
/// `Manager[T]` whose `with` gives `T`.
const MANAGERS: [&str; 5] = [
    "ContextManager",
    "AsyncContextManager",
    "AbstractContextManager",
    "AbstractAsyncContextManager",
    "_GeneratorContextManager",
];
/// What a `@contextmanager` function is annotated to return, `T` being what it yields.
const YIELDING: [&str; 6] = [
    "Iterator",
    "Generator",
    "Iterable",
    "AsyncIterator",
    "AsyncGenerator",
    "AsyncIterable",
];
/// How deep an evaluation nests, and how many steps one receiver may take.
const DEPTH: u8 = 40;
const FUEL: u32 = 2000;

/// A class: the module that defines it and its name there. A class of a module that is not
/// read keeps the module and name it is imported by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Class {
    pub module: String,
    pub name: String,
}

/// What an expression evaluates to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// An instance of the class.
    Instance(Class),
    /// The class itself.
    Type(Class),
    /// A function of `module`, or the method `name` of its class `class` bound to `receiver`.
    Function {
        module: String,
        class: Option<String>,
        name: String,
        receiver: Option<Class>,
    },
    /// A module.
    Module(String),
    /// A context manager whose `with` gives the value.
    Manager(Box<Value>),
}

/// An annotation, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ann {
    /// `Cls`, `mod.Cls`.
    Name(String),
    /// `Head[A, B]`.
    Generic(String, Vec<Ann>),
    /// `A | B`.
    Union(Vec<Ann>),
    /// `None`.
    None,
    /// Anything else.
    Unknown,
}

/// The expression a name or attribute is assigned, as far as a receiver reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    Name(String),
    /// The first parameter of the method the value is written in.
    This,
    Attr(Box<Val>, String),
    /// A call, with its first argument as a type when the callee may be `cast`.
    Call(Box<Val>, Option<Ann>),
    /// A declared type.
    Declared(Ann),
    /// `with value as`, `async with` when true.
    Entered(Box<Val>, bool),
    None,
    Unknown,
}

/// What a function or method declares.
#[derive(Clone, Debug)]
pub struct Method {
    pub returns: Ann,
    pub kind: Kind,
    /// `@contextmanager`, `@asynccontextmanager`.
    pub manager: bool,
    /// `@property`, `@cached_property`.
    pub property: bool,
}

/// What a class declares: its bases as written, its attributes and its methods.
#[derive(Clone, Debug, Default)]
pub struct ClassFacts {
    pub bases: Vec<Val>,
    pub attrs: HashMap<String, Vec<Val>>,
    pub methods: HashMap<String, Method>,
}

/// What a module declares.
#[derive(Debug, Default)]
pub struct Facts {
    pub classes: HashMap<String, ClassFacts>,
    pub functions: HashMap<String, Method>,
    pub names: HashMap<String, Vec<Val>>,
    targets: Targets,
}

fn annotation(expr: &Expr, depth: u8) -> Ann {
    if depth > 8 {
        return Ann::Unknown;
    }
    match expr {
        Expr::NoneLiteral(_) => Ann::None,
        Expr::StringLiteral(text) => crate::parse::expression(text.value.to_str())
            .map_or(Ann::Unknown, |parsed| {
                annotation(&parsed.syntax().body, depth + 1)
            }),
        Expr::BinOp(op) if op.op == Operator::BitOr => {
            let mut members = Vec::new();
            for side in [&op.left, &op.right] {
                match annotation(side, depth + 1) {
                    Ann::Union(inner) => members.extend(inner),
                    other => members.push(other),
                }
            }
            Ann::Union(members)
        }
        Expr::Subscript(sub) => {
            let Some(head) = dotted(&sub.value) else {
                return Ann::Unknown;
            };
            let args = match sub.slice.as_ref() {
                Expr::Tuple(items) => items
                    .elts
                    .iter()
                    .map(|item| annotation(item, depth + 1))
                    .collect(),
                item => vec![annotation(item, depth + 1)],
            };
            Ann::Generic(head, args)
        }
        expr => dotted(expr).map_or(Ann::Unknown, Ann::Name),
    }
}

fn value(expr: &Expr, depth: u8) -> Val {
    if depth > 16 {
        return Val::Unknown;
    }
    match expr {
        Expr::Name(name) => Val::Name(name.id.to_string()),
        Expr::Attribute(attr) => Val::Attr(
            Box::new(value(&attr.value, depth + 1)),
            attr.attr.to_string(),
        ),
        Expr::Call(call) => {
            let callee = value(&call.func, depth + 1);
            let cast = match &callee {
                Val::Name(name) | Val::Attr(_, name) if name == "cast" => call
                    .arguments
                    .args
                    .first()
                    .map(|first| annotation(first, 0)),
                _ => None,
            };
            Val::Call(Box::new(callee), cast)
        }
        Expr::Await(awaited) => value(&awaited.value, depth + 1),
        Expr::Named(named) => value(&named.value, depth + 1),
        Expr::NoneLiteral(_) => Val::None,
        _ => Val::Unknown,
    }
}

/// The value an [`Origin`] binds, and where it is written.
fn origin(origin: Origin<'_>) -> (Val, Option<u32>) {
    match origin {
        Origin::Declared(expr) => (Val::Declared(annotation(expr, 0)), None),
        Origin::Value(expr) => (value(expr, 0), Some(expr.start().to_u32())),
        Origin::Entered(expr, is_async) => (
            Val::Entered(Box::new(value(expr, 0)), is_async),
            Some(expr.start().to_u32()),
        ),
        Origin::Unknown => (Val::Unknown, None),
    }
}

/// The statements of `body` and of the blocks in it, not those of its functions and classes.
fn flat<'a>(body: &'a [Stmt], depth: u8, visit: &mut dyn FnMut(&'a Stmt)) {
    if depth > 32 {
        return;
    }
    for stmt in body {
        visit(stmt);
        match stmt {
            Stmt::If(node) => {
                flat(&node.body, depth + 1, visit);
                for clause in &node.elif_else_clauses {
                    flat(&clause.body, depth + 1, visit);
                }
            }
            Stmt::Try(node) => {
                flat(&node.body, depth + 1, visit);
                for handler in &node.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    flat(&handler.body, depth + 1, visit);
                }
                flat(&node.orelse, depth + 1, visit);
                flat(&node.finalbody, depth + 1, visit);
            }
            Stmt::With(node) => flat(&node.body, depth + 1, visit),
            Stmt::For(node) => {
                flat(&node.body, depth + 1, visit);
                flat(&node.orelse, depth + 1, visit);
            }
            Stmt::While(node) => {
                flat(&node.body, depth + 1, visit);
                flat(&node.orelse, depth + 1, visit);
            }
            _ => {}
        }
    }
}

fn decorated(function: &StmtFunctionDef, names: &[&str]) -> bool {
    function.decorator_list.iter().any(|decorator| {
        let expression = match &decorator.expression {
            Expr::Call(call) => call.func.as_ref(),
            expression => expression,
        };
        dotted(expression)
            .is_some_and(|name| names.contains(&name.rsplit('.').next().unwrap_or(&name)))
    })
}

fn method(function: &StmtFunctionDef, kind: Kind) -> Method {
    Method {
        returns: function
            .returns
            .as_deref()
            .map_or(Ann::Unknown, |returns| annotation(returns, 0)),
        kind,
        manager: decorated(function, &["contextmanager", "asynccontextmanager"]),
        property: decorated(function, &["property", "cached_property"]),
    }
}

/// Adds a definition of `name`: definitions that return different types (`@overload`)
/// leave the return type unknown.
fn define(methods: &mut HashMap<String, Method>, function: &StmtFunctionDef, kind: Kind) {
    if decorated(function, &["setter", "deleter"]) {
        return;
    }
    let found = method(function, kind);
    match methods.get_mut(function.name.as_str()) {
        Some(known) => {
            if known.returns != found.returns {
                known.returns = Ann::Unknown;
            }
        }
        None => {
            methods.insert(function.name.to_string(), found);
        }
    }
}

/// The names of the plain assignment targets of `stmt`, with what each is bound to.
fn assigned<'a>(stmt: &'a Stmt, bind: &mut dyn FnMut(&'a Expr, Val)) {
    match stmt {
        Stmt::Assign(node) => {
            for target in &node.targets {
                bind(target, value(&node.value, 0));
            }
        }
        Stmt::AnnAssign(node) => {
            let declared = annotation(&node.annotation, 0);
            let bare_final =
                matches!(&declared, Ann::Name(name) if name.rsplit('.').next() == Some("Final"));
            let bound = match &node.value {
                Some(value_) if bare_final => value(value_, 0),
                _ => Val::Declared(declared),
            };
            bind(&node.target, bound);
        }
        Stmt::With(node) => {
            for item in &node.items {
                if let Some(target) = &item.optional_vars {
                    bind(
                        target,
                        Val::Entered(Box::new(value(&item.context_expr, 0)), node.is_async),
                    );
                }
            }
        }
        Stmt::For(node) => bind(&node.target, Val::Unknown),
        Stmt::AugAssign(node) => bind(&node.target, Val::Unknown),
        _ => {}
    }
}

/// `val` as a method with the parameters `params` and the local names `locals` writes it:
/// its first parameter is the receiver, an annotated parameter its annotation.
fn inside(
    val: Val,
    this: &str,
    params: &HashMap<&str, Option<&Expr>>,
    locals: &HashSet<&str>,
) -> Val {
    match val {
        Val::Name(name) if name == this => Val::This,
        Val::Name(name) => match params.get(name.as_str()) {
            Some(Some(declared)) => Val::Declared(annotation(declared, 0)),
            Some(None) => Val::Unknown,
            None if locals.contains(name.as_str()) => Val::Unknown,
            None => Val::Name(name),
        },
        Val::Attr(base, attr) => Val::Attr(Box::new(inside(*base, this, params, locals)), attr),
        Val::Call(callee, cast) => Val::Call(Box::new(inside(*callee, this, params, locals)), cast),
        Val::Entered(entered, is_async) => {
            Val::Entered(Box::new(inside(*entered, this, params, locals)), is_async)
        }
        other => other,
    }
}

fn class_facts(class: &StmtClassDef) -> ClassFacts {
    let mut facts = ClassFacts {
        bases: class
            .bases()
            .iter()
            .map(|base| match base {
                Expr::Subscript(sub) => value(&sub.value, 0),
                base => value(base, 0),
            })
            .collect(),
        ..ClassFacts::default()
    };
    let mut methods = Vec::new();
    flat(&class.body, 0, &mut |stmt| match stmt {
        Stmt::FunctionDef(function) => methods.push(function),
        stmt => assigned(stmt, &mut |target, bound| {
            if let Expr::Name(name) = target {
                facts
                    .attrs
                    .entry(name.id.to_string())
                    .or_default()
                    .push(bound);
            }
        }),
    });
    for function in methods {
        let kind = method_kind(function);
        define(&mut facts.methods, function, kind);
        let parameters = &function.parameters;
        let Some(this) = parameters
            .posonlyargs
            .iter()
            .chain(&parameters.args)
            .next()
            .map(|first| first.parameter.name.as_str())
            .filter(|_| kind != Kind::Static)
        else {
            continue;
        };
        if !function_mentions_attribute(function, this) {
            continue;
        }
        let params: HashMap<&str, Option<&Expr>> = parameters
            .iter()
            .map(|parameter| {
                let declared = parameter.annotation().filter(|_| !parameter.is_variadic());
                (parameter.name().as_str(), declared)
            })
            .collect();
        let mut locals = HashSet::new();
        let mut written = Vec::new();
        flat(&function.body, 0, &mut |stmt| {
            assigned(stmt, &mut |target, bound| match target {
                Expr::Name(name) => {
                    locals.insert(name.id.as_str());
                }
                Expr::Attribute(attr) => {
                    if matches!(attr.value.as_ref(), Expr::Name(base) if base.id.as_str() == this) {
                        written.push((attr.attr.to_string(), bound));
                    }
                }
                Expr::Tuple(tuple) => {
                    for item in &tuple.elts {
                        if let Expr::Name(name) = item {
                            locals.insert(name.id.as_str());
                        }
                    }
                }
                _ => {}
            });
        });
        for (attr, bound) in written {
            facts
                .attrs
                .entry(attr)
                .or_default()
                .push(inside(bound, this, &params, &locals));
        }
    }
    facts
}

/// Whether the body of `function` may assign an attribute of `this`: its statements are
/// read only then.
fn function_mentions_attribute(function: &StmtFunctionDef, this: &str) -> bool {
    let mut found = false;
    flat(&function.body, 0, &mut |stmt| {
        let targets: &[Expr] = match stmt {
            Stmt::Assign(node) => &node.targets,
            Stmt::AnnAssign(node) => std::slice::from_ref(&*node.target),
            _ => &[],
        };
        found = found
            || targets.iter().any(|target| {
                matches!(target, Expr::Attribute(attr)
                    if matches!(attr.value.as_ref(), Expr::Name(base) if base.id.as_str() == this))
            });
    });
    found
}

impl Facts {
    /// What `source` declares, its imports being `targets`.
    pub fn of(source: &Source, targets: &Targets) -> Facts {
        let mut facts = Facts {
            targets: targets.clone(),
            ..Facts::default()
        };
        flat(source.body(), 0, &mut |stmt| match stmt {
            Stmt::FunctionDef(function) => define(&mut facts.functions, function, Kind::Function),
            stmt => assigned(stmt, &mut |target, bound| {
                if let Expr::Name(name) = target {
                    facts
                        .names
                        .entry(name.id.to_string())
                        .or_default()
                        .push(bound);
                }
            }),
        });
        // The outermost class of a name is the one other modules import.
        for class in source.classes() {
            if !facts.classes.contains_key(class.name.as_str()) {
                facts
                    .classes
                    .insert(class.name.to_string(), class_facts(class));
            }
        }
        facts
    }

    /// Whether the module binds `name` itself or imports it.
    fn has(&self, name: &str) -> bool {
        self.classes.contains_key(name)
            || self.functions.contains_key(name)
            || self.names.contains_key(name)
            || self.targets.0.get(name).is_some()
    }
}

/// Where an expression is evaluated: the module, the class `Self` and a method's first
/// parameter stand for, and for the checked module the scopes around it and the offset.
#[derive(Clone, Copy)]
struct Env<'e> {
    module: &'e str,
    this: Option<&'e Class>,
    stack: &'e [Scope],
    at: Option<u32>,
}

/// Evaluates the receivers of the calls of one checked module.
pub struct Resolver<'s, 'a> {
    /// The name of the checked module, empty without one.
    module: &'s str,
    source: &'s Source,
    targets: &'s Targets,
    helpers: Option<&'s HelperIndex>,
    own: &'s Dict<Helper>,
    scan: &'s Scan<'a>,
    local: OnceCell<Arc<Facts>>,
    read: RefCell<HashMap<String, Option<Arc<Facts>>>>,
    fuel: Cell<u32>,
}

impl<'s, 'a> Resolver<'s, 'a> {
    /// A resolver for `source`, the module `module` whose imports are `targets` and whose
    /// own helpers are `own`.
    pub fn new(
        source: &'s Source,
        module: Option<&'s str>,
        targets: &'s Targets,
        helpers: Option<&'s HelperIndex>,
        own: &'s Dict<Helper>,
        scan: &'s Scan<'a>,
    ) -> Self {
        Resolver {
            module: module.unwrap_or(""),
            source,
            targets,
            helpers,
            own,
            scan,
            local: OnceCell::new(),
            read: RefCell::default(),
            fuel: Cell::new(0),
        }
    }

    /// The helper that `receiver.name(...)` calls in the scopes `stack`: the method `name`
    /// of the class of `receiver`, or of the first of its bases that defines it.
    pub fn method(&self, receiver: &Expr, name: &str, stack: &[Scope]) -> Option<&'s Helper> {
        self.fuel.set(FUEL);
        let env = Env {
            module: self.module,
            this: None,
            stack,
            at: Some(receiver.start().to_u32()),
        };
        let (class, on_class) = match self.eval(&value(receiver, 0), env, 0)? {
            Value::Instance(class) => (class, false),
            Value::Type(class) => (class, true),
            _ => return None,
        };
        for base in self.mro(&class) {
            let key = format!("{}.{name}", base.name);
            let own = (base.module == self.module)
                .then(|| self.own.get(&key))
                .flatten();
            let found = own.or_else(|| {
                self.helpers
                    .and_then(|helpers| helpers.get(&base.module, &key))
            });
            if let Some(found) = found {
                return (!on_class || found.kind != Kind::Instance).then_some(found);
            }
            // A class that defines the method itself hides the helper of its bases.
            let overrides = self.facts(&base.module).is_some_and(|facts| {
                facts.classes.get(&base.name).is_some_and(|class| {
                    class.methods.contains_key(name) || class.attrs.contains_key(name)
                })
            });
            if overrides {
                return None;
            }
        }
        None
    }

    /// The class an expression of the scopes `stack` holds an instance of, as
    /// `module.Class`; for tests and `--explain`.
    pub fn class_of(&self, receiver: &Expr, stack: &[Scope]) -> Option<String> {
        self.fuel.set(FUEL);
        let env = Env {
            module: self.module,
            this: None,
            stack,
            at: Some(receiver.start().to_u32()),
        };
        match self.eval(&value(receiver, 0), env, 0)? {
            Value::Instance(class) => Some(format!("{}.{}", class.module, class.name)),
            _ => None,
        }
    }

    fn facts(&self, module: &str) -> Option<Arc<Facts>> {
        if module == self.module {
            return Some(Arc::clone(
                self.local
                    .get_or_init(|| Arc::new(Facts::of(self.source, self.targets))),
            ));
        }
        if KNOWN.contains(&module) || module.is_empty() {
            return None;
        }
        if let Some(known) = self.read.borrow().get(module) {
            return known.clone();
        }
        let found = self.helpers.and_then(|helpers| helpers.facts(module));
        self.read
            .borrow_mut()
            .insert(module.to_string(), found.clone());
        found
    }

    /// Whether another step may be taken at `depth`.
    fn step(&self, depth: u8) -> Option<()> {
        let left = self.fuel.get().checked_sub(1)?;
        self.fuel.set(left);
        (depth < DEPTH).then_some(())
    }

    #[recursive::recursive]
    fn eval(&self, val: &Val, env: Env<'_>, depth: u8) -> Option<Value> {
        self.step(depth)?;
        match val {
            Val::Name(name) => match self.local_name(name, env, depth + 1) {
                Some(bound) => bound,
                None => self.global(env.module, name, depth + 1),
            },
            Val::This => env.this.cloned().map(Value::Instance),
            Val::Attr(base, attr) => {
                let base = self.eval(base, env, depth + 1)?;
                self.member(&base, attr, depth + 1)
            }
            Val::Call(callee, cast) => {
                let callee = self.eval(callee, env, depth + 1)?;
                self.call(&callee, cast.as_ref(), env, depth + 1)
            }
            Val::Declared(declared) => self.declared(declared, env, depth + 1),
            Val::Entered(entered, is_async) => {
                let entered = self.eval(entered, env, depth + 1)?;
                self.enter(&entered, *is_async, depth + 1)
            }
            Val::None | Val::Unknown => None,
        }
    }

    /// The one value every binding of `bound` gives, `None` ones aside (`T | None` is `T`).
    fn agreed(&self, bound: &[(Val, Option<u32>)], env: Env<'_>, depth: u8) -> Option<Value> {
        // A declaration states the type whatever is assigned.
        if let Some((declared, _)) = bound
            .iter()
            .find(|(val, _)| matches!(val, Val::Declared(_)))
        {
            return self.eval(declared, env, depth);
        }
        let mut found: Option<Value> = None;
        for (val, at) in bound {
            if matches!(val, Val::None) {
                continue;
            }
            let env = Env {
                at: at.or(env.at),
                ..env
            };
            let value = self.eval(val, env, depth)?;
            if found.as_ref().is_some_and(|known| *known != value) {
                return None;
            }
            found = Some(value);
        }
        found
    }

    fn decl(&self, bound: &[Val], env: Env<'_>, depth: u8) -> Option<Value> {
        let bound: Vec<(Val, Option<u32>)> = bound.iter().map(|val| (val.clone(), None)).collect();
        self.agreed(&bound, env, depth)
    }

    /// What `name` is bound to by a function around the expression, `None` when none of
    /// them binds it: then it is a name of the module.
    fn local_name(&self, name: &str, env: Env<'_>, depth: u8) -> Option<Option<Value>> {
        if env.module != self.module {
            return None;
        }
        for (i, &scope) in env.stack.iter().enumerate().rev() {
            let Node::Function(function) = self.scan.scopes[scope] else {
                continue;
            };
            let class =
                i.checked_sub(1)
                    .and_then(|outer| match self.scan.scopes[env.stack[outer]] {
                        Node::Class(class) => Some(class),
                        _ => None,
                    });
            if let Some(class) = class {
                let parameters = &function.parameters;
                let first = parameters.posonlyargs.iter().chain(&parameters.args).next();
                let kind = method_kind(function);
                if kind != Kind::Static
                    && first.is_some_and(|first| first.parameter.name.as_str() == name)
                {
                    let class = Class {
                        module: self.module.to_string(),
                        name: class.name.to_string(),
                    };
                    let on_class = kind == Kind::Class
                        || matches!(function.name.as_str(), "__new__" | "__init_subclass__");
                    return Some(Some(if on_class {
                        Value::Type(class)
                    } else {
                        Value::Instance(class)
                    }));
                }
            }
            // A name of an enclosing function can be rebound before the inner one runs.
            let at = env.at.filter(|_| i + 1 == env.stack.len());
            if let Some(origins) = self.scan.bound(scope, name, at) {
                let bound: Vec<(Val, Option<u32>)> = origins.into_iter().map(origin).collect();
                let this = class.map(|class| Class {
                    module: self.module.to_string(),
                    name: class.name.to_string(),
                });
                let env = Env {
                    stack: &env.stack[..=i],
                    this: this.as_ref(),
                    ..env
                };
                return Some(self.agreed(&bound, env, depth));
            }
        }
        None
    }

    /// What `name` is at the top of `module`. A module that is not read gives the class
    /// `module.name`, so that a helper registered by that path is still found.
    #[recursive::recursive]
    fn global(&self, module: &str, name: &str, depth: u8) -> Option<Value> {
        self.step(depth)?;
        let Some(facts) = self.facts(module) else {
            return Some(Value::Type(Class {
                module: module.to_string(),
                name: name.to_string(),
            }));
        };
        if facts.classes.contains_key(name) {
            return Some(Value::Type(Class {
                module: module.to_string(),
                name: name.to_string(),
            }));
        }
        if facts.functions.contains_key(name) {
            return Some(Value::Function {
                module: module.to_string(),
                class: None,
                name: name.to_string(),
                receiver: None,
            });
        }
        let env = Env {
            module,
            this: None,
            stack: &[],
            at: None,
        };
        if let Some(bound) = facts.names.get(name) {
            return self.decl(bound, env, depth + 1);
        }
        let (functions, modules) = &facts.targets;
        if let Some((target, original)) = functions.get(name) {
            // `from . import db` in a package names its submodule, not itself.
            let itself = target == module && original == name;
            return match self.facts(target) {
                Some(found) if !itself && found.has(original) => {
                    self.global(target, original, depth + 1)
                }
                Some(_) => {
                    let submodule = format!("{target}.{original}");
                    self.facts(&submodule)
                        .is_some()
                        .then_some(Value::Module(submodule))
                }
                // A package that is not read: its submodules are not looked for.
                None => self.global(target, original, depth + 1),
            };
        }
        if let Some(target) = modules.get(name) {
            return Some(Value::Module(target.clone()));
        }
        // `import a.b` binds `a`.
        let prefix = format!("{name}.");
        let package = modules.keys().any(|local| local.starts_with(&prefix));
        package.then(|| Value::Module(name.to_string()))
    }

    #[recursive::recursive]
    fn member(&self, base: &Value, attr: &str, depth: u8) -> Option<Value> {
        self.step(depth)?;
        match base {
            Value::Module(module) => {
                let found = self.facts(module);
                if found.as_ref().is_some_and(|facts| facts.has(attr)) {
                    return self.global(module, attr, depth + 1);
                }
                // A package does not have to import its submodule for `import a.b` to
                // bind it.
                let submodule = format!("{module}.{attr}");
                if self.facts(&submodule).is_some() {
                    Some(Value::Module(submodule))
                } else if found.is_none() {
                    self.global(module, attr, depth + 1)
                } else {
                    None
                }
            }
            Value::Instance(class) | Value::Type(class) => {
                if self.facts(&class.module).is_none() {
                    // A name of a module that is not read may be a module or a class.
                    return matches!(base, Value::Type(_)).then(|| {
                        Value::Type(Class {
                            module: format!("{}.{}", class.module, class.name),
                            name: attr.to_string(),
                        })
                    });
                }
                for owner in self.mro(class) {
                    let facts = self.facts(&owner.module)?;
                    let found = facts.classes.get(&owner.name)?;
                    let env = Env {
                        module: &owner.module,
                        this: Some(class),
                        stack: &[],
                        at: None,
                    };
                    if let Some(bound) = found.attrs.get(attr) {
                        return self.decl(bound, env, depth + 1);
                    }
                    if let Some(method) = found.methods.get(attr) {
                        if method.property {
                            return self.declared(&method.returns, env, depth + 1);
                        }
                        return Some(Value::Function {
                            module: owner.module.clone(),
                            class: Some(owner.name.clone()),
                            name: attr.to_string(),
                            receiver: Some(class.clone()),
                        });
                    }
                }
                None
            }
            Value::Function { .. } | Value::Manager(_) => None,
        }
    }

    fn call(&self, callee: &Value, cast: Option<&Ann>, env: Env<'_>, depth: u8) -> Option<Value> {
        self.step(depth)?;
        match callee {
            Value::Type(class) if TYPING.contains(&class.module.as_str()) => {
                (class.name == "cast").then_some(())?;
                self.declared(cast?, env, depth + 1)
            }
            Value::Type(class) => Some(Value::Instance(class.clone())),
            Value::Function {
                module,
                class,
                name,
                receiver,
            } => {
                let facts = self.facts(module)?;
                let method = match class {
                    Some(class) => facts.classes.get(class)?.methods.get(name)?,
                    None => facts.functions.get(name)?,
                };
                let env = Env {
                    module,
                    this: receiver.as_ref(),
                    stack: &[],
                    at: None,
                };
                if method.manager {
                    let Ann::Generic(head, args) = &method.returns else {
                        return None;
                    };
                    YIELDING
                        .contains(&head.rsplit('.').next().unwrap_or(head))
                        .then_some(())?;
                    let yielded = self.declared(args.first()?, env, depth + 1)?;
                    return Some(Value::Manager(Box::new(yielded)));
                }
                self.declared(&method.returns, env, depth + 1)
            }
            _ => None,
        }
    }

    /// What `with entered as` binds.
    fn enter(&self, entered: &Value, is_async: bool, depth: u8) -> Option<Value> {
        match entered {
            Value::Manager(inner) => Some((**inner).clone()),
            Value::Instance(_) => {
                let name = if is_async { "__aenter__" } else { "__enter__" };
                let method = self.member(entered, name, depth + 1)?;
                let env = Env {
                    module: self.module,
                    this: None,
                    stack: &[],
                    at: None,
                };
                self.call(&method, None, env, depth + 1)
            }
            _ => None,
        }
    }

    /// The value of something annotated `declared`.
    #[recursive::recursive]
    fn declared(&self, declared: &Ann, env: Env<'_>, depth: u8) -> Option<Value> {
        self.step(depth)?;
        let named = |name: &str| -> Option<Value> {
            let mut parts = name.split('.');
            let mut val = Val::Name(parts.next()?.to_string());
            for part in parts {
                val = Val::Attr(Box::new(val), part.to_string());
            }
            self.eval(&val, env, depth + 1)
        };
        match declared {
            Ann::Name(name) => match named(name)? {
                Value::Type(class) if TYPING.contains(&class.module.as_str()) => {
                    (class.name == "Self").then_some(())?;
                    env.this.cloned().map(Value::Instance)
                }
                Value::Type(class) => Some(Value::Instance(class)),
                _ => None,
            },
            Ann::Union(members) => {
                let mut found: Option<Value> = None;
                for member in members {
                    if matches!(member, Ann::None) {
                        continue;
                    }
                    let value = self.declared(member, env, depth + 1)?;
                    if found.as_ref().is_some_and(|known| *known != value) {
                        return None;
                    }
                    found = Some(value);
                }
                found
            }
            Ann::Generic(head, args) => {
                if head == "type" {
                    return match self.declared(args.first()?, env, depth + 1)? {
                        Value::Instance(class) => Some(Value::Type(class)),
                        _ => None,
                    };
                }
                let Value::Type(class) = named(head)? else {
                    return None;
                };
                let name = class.name.as_str();
                if TYPING.contains(&class.module.as_str()) {
                    return if TRANSPARENT.contains(&name) {
                        self.declared(args.first()?, env, depth + 1)
                    } else if name == "Union" {
                        self.declared(&Ann::Union(args.clone()), env, depth + 1)
                    } else if name == "Type" {
                        match self.declared(args.first()?, env, depth + 1)? {
                            Value::Instance(class) => Some(Value::Type(class)),
                            _ => None,
                        }
                    } else if MANAGERS.contains(&name) {
                        let inner = self.declared(args.first()?, env, depth + 1)?;
                        Some(Value::Manager(Box::new(inner)))
                    } else {
                        None
                    };
                }
                if class.module == "contextlib" && MANAGERS.contains(&name) {
                    let inner = self.declared(args.first()?, env, depth + 1)?;
                    return Some(Value::Manager(Box::new(inner)));
                }
                Some(Value::Instance(class))
            }
            Ann::None | Ann::Unknown => None,
        }
    }

    /// `class` and its bases, each before its own bases and once: the order methods are
    /// looked up in when no class is inherited twice.
    fn mro(&self, class: &Class) -> Vec<Class> {
        let mut order = vec![class.clone()];
        let mut next = 0;
        // Depth first: the bases of a class go right after it.
        let mut pending: Vec<Class> = Vec::new();
        while next < order.len() && order.len() < 64 {
            let current = order[next].clone();
            next += 1;
            let Some(facts) = self.facts(&current.module) else {
                continue;
            };
            let Some(found) = facts.classes.get(&current.name) else {
                continue;
            };
            let env = Env {
                module: &current.module,
                this: None,
                stack: &[],
                at: None,
            };
            pending.clear();
            for base in &found.bases {
                if let Some(Value::Type(base)) = self.eval(base, env, 1) {
                    if !order.contains(&base) && !pending.contains(&base) {
                        pending.push(base);
                    }
                }
            }
            order.splice(next..next, pending.drain(..));
        }
        order
    }
}
