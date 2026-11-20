//! The class of a call's receiver, read from declarations the way a type checker reads
//! them: annotations, `x = Cls(...)`, attributes declared on a class or its bases, return
//! annotations, `with ... as`, `isinstance`. Nothing is inferred from a function body,
//! nothing is imported or run; what no declaration states is unknown and the call is
//! skipped.
//!
//! [`Facts`] is what one module declares, owned so that the index keeps it per module.
//! [`Resolver`] evaluates the receiver of a call of the checked module against them: a
//! name is what the bindings that reach the call give it ([`crate::flow`]), an attribute
//! what the class of its base or one of that class's bases declares.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use ruff_python_ast::{Alias, Expr, Operator, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::calls::{method_kind, Helper, Kind};
use crate::flow;
use crate::index::{imports_of, HelperIndex};
use crate::module::Import;
use crate::scopes::{dotted, Node, Origin, Scan, Scope};
use crate::types::info::Dict;
use crate::types::symbols::Source;

/// What a name of a module that is read by name, never from its source, means to a
/// receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// `cast(T, value)`.
    Cast,
    /// `Self`.
    Receiver,
    /// `Wrapper[T]` that is `T`: `Optional`, `ClassVar`, `Final`, `Annotated`, ...
    Transparent,
    /// `Union[A, B]`.
    Union,
    /// `Type[T]`.
    TypeOf,
    /// `Manager[T]` whose `with` gives `T`.
    Manager,
}

/// The special forms, by module and name; `typing` stands for `typing_extensions` too.
const FORMS: [(&str, &str, Form); 16] = [
    ("typing", "cast", Form::Cast),
    ("typing", "Self", Form::Receiver),
    ("typing", "Optional", Form::Transparent),
    ("typing", "ClassVar", Form::Transparent),
    ("typing", "Final", Form::Transparent),
    ("typing", "Annotated", Form::Transparent),
    ("typing", "Required", Form::Transparent),
    ("typing", "NotRequired", Form::Transparent),
    ("typing", "ReadOnly", Form::Transparent),
    ("typing", "Union", Form::Union),
    ("typing", "Type", Form::TypeOf),
    ("typing", "ContextManager", Form::Manager),
    ("typing", "AsyncContextManager", Form::Manager),
    ("contextlib", "AbstractContextManager", Form::Manager),
    ("contextlib", "AbstractAsyncContextManager", Form::Manager),
    ("contextlib", "_GeneratorContextManager", Form::Manager),
];
/// Modules whose names are read by name: every other name of theirs is unknown.
const KNOWN: [&str; 7] = [
    "typing",
    "typing_extensions",
    "contextlib",
    "collections.abc",
    "builtins",
    "abc",
    "sqlty",
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
/// The module of the class that stands for a base that is not known.
const NOWHERE: &str = "\0";

/// A class: the module that defines it and its name there. A class of a module that is not
/// read keeps the module and name it is imported by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Class {
    pub module: String,
    pub name: String,
}

impl Class {
    fn new(module: &str, name: &str) -> Self {
        Class {
            module: module.to_string(),
            name: name.to_string(),
        }
    }

    /// What the name means when its module is one read by name; `Err` for a name of such
    /// a module that means nothing here.
    fn form(&self) -> Option<Result<Form, ()>> {
        let module = match self.module.as_str() {
            "typing_extensions" => "typing",
            module => module,
        };
        KNOWN.contains(&module).then(|| {
            FORMS
                .iter()
                .find(|(owner, name, _)| *owner == module && *name == self.name)
                .map(|(_, _, form)| *form)
                .ok_or(())
        })
    }
}

/// What an expression evaluates to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// An instance of the class.
    Instance(Class),
    /// The class itself.
    Type(Class),
    /// A name of a module that is not read: a class, a function or a module, known by
    /// the path it is imported by. It is called and annotated with as a class would be,
    /// so that a helper registered by that path is found.
    Unresolved(Class),
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

/// What a name or attribute is bound to, as far as a receiver reads it.
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
    /// The class of this name, defined at the top of the module.
    Class(String),
    /// The function of this name, defined at the top of the module.
    Function(String),
    /// `from module import original as local`, the module named in full.
    From {
        module: String,
        original: String,
        local: String,
    },
    /// A module, as `import` binds it.
    Module(String),
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

/// What a module declares at its top: every binding of every name, its classes and its
/// functions. Classes defined in a function or in another class are not modelled.
#[derive(Debug, Default)]
pub struct Facts {
    pub classes: HashMap<String, ClassFacts>,
    pub functions: HashMap<String, Method>,
    pub names: HashMap<String, Vec<Val>>,
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

/// What the import `alias` of `stmt` binds `name` to, in the module `module`.
fn imported(stmt: &Stmt, alias: &Alias, name: &str, module: Option<&str>) -> Val {
    let entry = match stmt {
        Stmt::ImportFrom(from) => Import {
            local: name.to_string(),
            original: alias.name.to_string(),
            module: Some(
                from.module
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
            ),
            level: from.level,
        },
        _ => Import {
            local: alias.asname.as_ref().unwrap_or(&alias.name).to_string(),
            original: alias.name.to_string(),
            module: None,
            level: 0,
        },
    };
    let (functions, modules) = imports_of(&[entry], module);
    if let Some((module, original)) = functions.get(name) {
        return Val::From {
            module: module.clone(),
            original: original.clone(),
            local: name.to_string(),
        };
    }
    // `import a.b` binds `a`.
    Val::Module(modules.get(name).map_or(name, String::as_str).to_string())
}

/// What a binding of `name` gives it in the module `module`, and where the expression to
/// evaluate is written. A class and a function are followed at the top of a module only:
/// in a function they hide the name.
fn bound(origin: Origin<'_>, name: &str, top: bool, module: Option<&str>) -> (Val, Option<u32>) {
    match origin {
        Origin::Declared(declared, _) => (Val::Declared(annotation(declared, 0)), None),
        Origin::Value(expr) => (value(expr, 0), Some(expr.start().to_u32())),
        Origin::Entered(expr, is_async) => (
            Val::Entered(Box::new(value(expr, 0)), is_async),
            Some(expr.start().to_u32()),
        ),
        Origin::Class(_) if top => (Val::Class(name.to_string()), None),
        Origin::Function(_) if top => (Val::Function(name.to_string()), None),
        Origin::Import(stmt, alias) => (imported(stmt, alias, name, module), None),
        Origin::Class(_) | Origin::Function(_) | Origin::Parameter | Origin::Unknown => {
            (Val::Unknown, None)
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

/// The first parameter of a method that has a receiver.
fn receiver_name(function: &StmtFunctionDef) -> Option<&str> {
    let parameters = &function.parameters;
    parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .next()
        .map(|first| first.parameter.name.as_str())
        .filter(|_| method_kind(function) != Kind::Static)
}

/// `val` as the method `scope` of the module `module` writes it: its first parameter is
/// the receiver, a parameter it never binds again its annotation, a name it imports
/// what it imports, any other name it binds unknown.
fn inside(val: Val, scan: &Scan<'_>, scope: Scope, this: &str, module: Option<&str>) -> Val {
    match val {
        Val::Name(name) if name == this => Val::This,
        Val::Name(name) => match scan.bindings(scope, &name) {
            Some([only]) => match only.origin {
                Origin::Declared(declared, _) => Val::Declared(annotation(declared, 0)),
                Origin::Import(stmt, alias) => imported(stmt, alias, &name, module),
                _ => Val::Unknown,
            },
            Some(_) => Val::Unknown,
            None => Val::Name(name),
        },
        Val::Attr(base, attr) => {
            Val::Attr(Box::new(inside(*base, scan, scope, this, module)), attr)
        }
        Val::Call(callee, cast) => {
            Val::Call(Box::new(inside(*callee, scan, scope, this, module)), cast)
        }
        Val::Entered(entered, is_async) => Val::Entered(
            Box::new(inside(*entered, scan, scope, this, module)),
            is_async,
        ),
        other => other,
    }
}

fn class_facts(
    class: &StmtClassDef,
    scope: Scope,
    scan: &Scan<'_>,
    module: Option<&str>,
) -> ClassFacts {
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
    for (name, bindings) in scan.bound_in(scope) {
        let mut vals = Vec::new();
        for binding in bindings {
            match binding.origin {
                Origin::Function(function) => {
                    define(&mut facts.methods, function, method_kind(function));
                }
                origin => vals.push(bound(origin, name, false, module).0),
            }
        }
        if !vals.is_empty() {
            // A name that is a method too is neither.
            if facts.methods.contains_key(name) {
                vals.push(Val::Unknown);
            }
            facts.attrs.insert(name.to_string(), vals);
        }
    }
    for (name, written) in scan.attributes_of(scope) {
        for (origin, method) in written {
            let Node::Function(function) = scan.scopes[*method] else {
                continue;
            };
            let Some(this) = receiver_name(function) else {
                continue;
            };
            let written = bound(*origin, name, false, module).0;
            let val = inside(written, scan, *method, this, module);
            facts.attrs.entry(name.to_string()).or_default().push(val);
        }
    }
    facts
}

impl Facts {
    /// What `source` declares as the module `module`.
    pub fn of(source: &Source, module: Option<&str>) -> Facts {
        let scan = Scan::new(source.body(), |_| false);
        Self::from_scan(&scan, module)
    }

    /// What the module `scan` walked declares, its relative imports read as those of
    /// `module`.
    pub fn from_scan(scan: &Scan<'_>, module: Option<&str>) -> Facts {
        let mut facts = Facts::default();
        for (name, bindings) in scan.bound_in(0) {
            let mut vals = Vec::new();
            let mut classes = 0;
            for binding in bindings {
                match binding.origin {
                    Origin::Function(function) => {
                        define(&mut facts.functions, function, Kind::Function);
                    }
                    Origin::Class(_) => classes += 1,
                    _ => {}
                }
                vals.push(bound(binding.origin, name, true, module).0);
            }
            // Two classes of one name: which one a use means is not followed.
            if classes > 1 {
                vals.push(Val::Unknown);
            }
            facts.names.insert(name.to_string(), vals);
        }
        for (scope, node) in scan.scopes.iter().enumerate() {
            if let (Node::Class(class), 0) = (node, scan.parents[scope]) {
                if !facts.classes.contains_key(class.name.as_str()) {
                    facts.classes.insert(
                        class.name.to_string(),
                        class_facts(class, scope, scan, module),
                    );
                }
            }
        }
        facts
    }
}

/// Where an expression is evaluated: the module, the class `Self` and a method's first
/// parameter stand for, and for the checked module the scopes around it and its offset.
#[derive(Clone, Copy)]
struct Env<'e> {
    module: &'e str,
    this: Option<&'e Class>,
    stack: &'e [Scope],
    at: Option<u32>,
}

impl<'e> Env<'e> {
    /// The top of `module`, where the order of its statements is not followed.
    fn top(module: &'e str) -> Self {
        Env {
            module,
            this: None,
            stack: &[],
            at: None,
        }
    }
}

/// Evaluates the receivers of the calls of one checked module.
pub struct Resolver<'s, 'a> {
    /// The name of the checked module, empty without one.
    module: &'s str,
    /// The name its relative imports are read by.
    named: Option<&'s str>,
    source: &'s Source,
    helpers: Option<&'s HelperIndex>,
    own: &'s Dict<Helper>,
    scan: &'s Scan<'a>,
    local: OnceCell<Arc<Facts>>,
    read: RefCell<HashMap<String, Option<Arc<Facts>>>>,
    orders: RefCell<HashMap<Class, Option<Rc<Vec<Class>>>>>,
    fuel: Cell<u32>,
}

impl<'s, 'a> Resolver<'s, 'a> {
    /// A resolver for `source`, the module `module` whose own helpers are `own`, walked
    /// by `scan`.
    pub fn new(
        source: &'s Source,
        module: Option<&'s str>,
        helpers: Option<&'s HelperIndex>,
        own: &'s Dict<Helper>,
        scan: &'s Scan<'a>,
    ) -> Self {
        let named = module.or(source.module.as_deref());
        Resolver {
            module: module.map_or("", |module| {
                module.strip_suffix(".__init__").unwrap_or(module)
            }),
            named,
            source,
            helpers,
            own,
            scan,
            local: OnceCell::new(),
            read: RefCell::default(),
            orders: RefCell::default(),
            fuel: Cell::new(0),
        }
    }

    /// The helper that `receiver.name(...)` calls in the scopes `stack`: the method `name`
    /// of the class of `receiver`, or of the first class of its method resolution order
    /// that defines it.
    pub fn method(&self, receiver: &Expr, name: &str, stack: &[Scope]) -> Option<&'s Helper> {
        let (class, on_class) = match self.value_of(receiver, stack)? {
            Value::Instance(class) => (class, false),
            Value::Type(class) | Value::Unresolved(class) => (class, true),
            _ => return None,
        };
        for base in self.order(&class)?.iter() {
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
            // A base that is not read may define the method, and a class that defines it
            // itself hides the helper of its bases.
            let facts = self.facts(&base.module)?;
            let known = facts.classes.get(&base.name)?;
            if known.methods.contains_key(name) || known.attrs.contains_key(name) {
                return None;
            }
        }
        None
    }

    /// The class an expression of the scopes `stack` holds an instance of, as
    /// `module.Class`.
    pub fn class_of(&self, receiver: &Expr, stack: &[Scope]) -> Option<String> {
        match self.value_of(receiver, stack)? {
            Value::Instance(class) => Some(format!("{}.{}", class.module, class.name)),
            _ => None,
        }
    }

    fn value_of(&self, receiver: &Expr, stack: &[Scope]) -> Option<Value> {
        self.fuel.set(FUEL);
        let env = Env {
            module: self.module,
            this: None,
            stack,
            at: Some(receiver.start().to_u32()),
        };
        self.eval(&value(receiver, 0), env, 0)
    }

    fn facts(&self, module: &str) -> Option<Arc<Facts>> {
        if module == self.module {
            return Some(Arc::clone(self.local.get_or_init(|| {
                // The module the index holds is this one when its text is: what the walk
                // found is kept with it, for the modules that import it.
                let read = || Facts::from_scan(self.scan, self.named);
                let kept = self
                    .helpers
                    .and_then(|helpers| helpers.module(self.module))
                    .filter(|kept| {
                        Arc::ptr_eq(&kept.text, &self.source.text)
                            && Some(kept.name()) == self.named
                    })
                    .and_then(|kept| kept.facts_or(|| Some(read())).cloned());
                kept.unwrap_or_else(|| Arc::new(read()))
            })));
        }
        if KNOWN.contains(&module) || module.is_empty() || module == NOWHERE {
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
            Val::Class(name) => Some(Value::Type(Class::new(env.module, name))),
            Val::Function(name) => Some(Value::Function {
                module: env.module.to_string(),
                class: None,
                name: name.clone(),
                receiver: None,
            }),
            Val::From {
                module,
                original,
                local,
            } => self.imported(env.module, module, original, local, depth + 1),
            Val::Module(module) => Some(Value::Module(module.clone())),
            Val::None | Val::Unknown => None,
        }
    }

    /// The one value every binding of `bound` gives, `None` ones aside (`T | None` is
    /// `T`). With `declared`, a declaration states the type whatever else is assigned.
    fn agreed(
        &self,
        bound: &[(Val, Option<u32>)],
        declared: bool,
        env: Env<'_>,
        depth: u8,
    ) -> Option<Value> {
        let stated = bound
            .iter()
            .find(|(val, _)| declared && matches!(val, Val::Declared(_)));
        if let Some((stated, _)) = stated {
            return self.eval(stated, Env { at: None, ..env }, depth);
        }
        let mut found: Option<Value> = None;
        for (val, at) in bound {
            if matches!(val, Val::None) {
                continue;
            }
            let value = self.eval(val, Env { at: *at, ..env }, depth)?;
            if found.as_ref().is_some_and(|known| *known != value) {
                return None;
            }
            found = Some(value);
        }
        found
    }

    /// What the bindings of a name or attribute, read in no order, give it.
    fn decl(&self, bound: &[Val], env: Env<'_>, depth: u8) -> Option<Value> {
        let bound: Vec<(Val, Option<u32>)> = bound.iter().map(|val| (val.clone(), None)).collect();
        self.agreed(&bound, true, env, depth)
    }

    /// What `name` is bound to by a scope around the expression; `None` when none of them
    /// binds it, or when it is a name of the module read from a function: then it is what
    /// the module binds it to, in no order.
    fn local_name(&self, name: &str, env: Env<'_>, depth: u8) -> Option<Option<Value>> {
        if env.module != self.module {
            return None;
        }
        if let Some(at) = env.at {
            let around = env.stack.iter().chain(std::iter::once(&0));
            if around
                .into_iter()
                .any(|&scope| self.scan.shadowed(scope, name, at))
            {
                return Some(None);
            }
        }
        // The innermost scope runs its statements in order; a scope around it has run
        // any of them by the time a function in it is called.
        let innermost = env.stack.last().copied().unwrap_or(0);
        for (i, &scope) in env.stack.iter().enumerate().rev() {
            let node = self.scan.scopes[scope];
            let body: &[Stmt] = match node {
                Node::Function(function) => &function.body,
                Node::Class(class) if scope == innermost => &class.body,
                Node::Class(_) | Node::Module => continue,
            };
            let bindings = self.scan.bindings(scope, name);
            let outer = i.checked_sub(1).map(|outer| env.stack[outer]);
            let class = outer.and_then(|outer| match self.scan.scopes[outer] {
                Node::Class(class) => Some((class, self.scan.parents[outer] == 0)),
                _ => None,
            });
            if let (Node::Function(function), Some((class, top))) = (node, class) {
                let rebound = bindings.is_some_and(|bindings| bindings.len() > 1);
                if receiver_name(function) == Some(name) && !rebound {
                    // A class in a function or in another class is not modelled.
                    let class = Class::new(self.module, class.name.as_str());
                    let on_class = method_kind(function) == Kind::Class
                        || matches!(function.name.as_str(), "__new__" | "__init_subclass__");
                    return Some(top.then_some(match on_class {
                        true => Value::Type(class),
                        false => Value::Instance(class),
                    }));
                }
            }
            let Some(bindings) = bindings else {
                continue;
            };
            let at = env.at.filter(|_| scope == innermost);
            let reach = at.and_then(|at| flow::reaching(body, bindings, name, at));
            let this = class.map(|(class, _)| Class::new(self.module, class.name.as_str()));
            let env = Env {
                stack: &env.stack[..=i],
                this: this.as_ref(),
                ..env
            };
            return Some(match reach {
                Some(reach) => self.reached(name, &reach, false, env, depth),
                None => {
                    let bound: Vec<_> = bindings
                        .iter()
                        .map(|binding| bound(binding.origin, name, false, self.named))
                        .collect();
                    self.agreed(&bound, true, env, depth)
                }
            });
        }
        if env.stack.iter().any(|&scope| !self.scan.is_class(scope)) {
            return None;
        }
        let bindings = self.scan.bindings(0, name)?;
        let reach = flow::reaching(self.source.body(), bindings, name, env.at?)?;
        let env = Env { stack: &[], ..env };
        Some(self.reached(name, &reach, true, env, depth))
    }

    /// What `name` holds where the bindings `reach` are in effect. An `isinstance` test
    /// states the class of a name nothing else is known of (no annotation, `object`,
    /// `Any`, a value that is not followed), and of one known to be of a base of that
    /// class; a subclass of the class stays what it is. A name known to be of an
    /// unrelated class would be of both, which is not followed.
    fn reached(
        &self,
        name: &str,
        reach: &flow::Reach<'_>,
        top: bool,
        env: Env<'_>,
        depth: u8,
    ) -> Option<Value> {
        let bound: Vec<_> = reach
            .origins
            .iter()
            .map(|(origin, _)| bound(*origin, name, top, self.named))
            .collect();
        let rebound = |scope: &Scope| self.scan.bindings(*scope, "isinstance").is_some();
        let tested = reach
            .narrowed
            .filter(|_| !env.stack.iter().chain(std::iter::once(&0)).any(rebound));
        let Some(tested) = tested else {
            return self.agreed(&bound, false, env, depth);
        };
        let at = Some(tested.start().to_u32());
        let class = self.eval(&value(tested, 0), Env { at, ..env }, depth)?;
        let (Value::Type(class) | Value::Unresolved(class)) = class else {
            return None;
        };
        let mut held: Option<Class> = None;
        let mut unknown = false;
        for (val, at) in &bound {
            // An annotation that is not followed may still state a class (an alias of
            // a union, a type variable): only `object` and `Any` state none.
            let spelled = match val {
                Val::Declared(Ann::Name(name)) => {
                    name != "object" && name.rsplit('.').next() != Some("Any")
                }
                Val::Declared(_) => true,
                _ => false,
            };
            match self.eval(val, Env { at: *at, ..env }, depth) {
                Some(Value::Instance(known)) => {
                    if held.as_ref().is_some_and(|held| *held != known) {
                        return None;
                    }
                    held = Some(known);
                }
                None if !spelled && !matches!(val, Val::None) => unknown = true,
                _ => return None,
            }
        }
        let held = match held {
            Some(_) if unknown => return None,
            Some(held) => held,
            None => return Some(Value::Instance(class)),
        };
        if self.order(&held)?.contains(&class) {
            return Some(Value::Instance(held));
        }
        self.order(&class)?
            .contains(&held)
            .then_some(Value::Instance(class))
    }

    /// What `name` is at the top of `module`, whatever the order of its statements. A
    /// module that is not read gives the name by its path.
    #[recursive::recursive]
    fn global(&self, module: &str, name: &str, depth: u8) -> Option<Value> {
        self.step(depth)?;
        let Some(facts) = self.facts(module) else {
            return Some(Value::Unresolved(Class::new(module, name)));
        };
        self.decl(facts.names.get(name)?, Env::top(module), depth + 1)
    }

    /// What `from target import original as local` binds in `module`.
    fn imported(
        &self,
        module: &str,
        target: &str,
        original: &str,
        local: &str,
        depth: u8,
    ) -> Option<Value> {
        // `from . import db` in a package names its submodule, not itself.
        let itself = target == module && original == local;
        match self.facts(target) {
            Some(found) if !itself && found.names.contains_key(original) => {
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
        }
    }

    #[recursive::recursive]
    fn member(&self, base: &Value, attr: &str, depth: u8) -> Option<Value> {
        self.step(depth)?;
        match base {
            Value::Module(module) => {
                let found = self.facts(module);
                if found
                    .as_ref()
                    .is_some_and(|facts| facts.names.contains_key(attr))
                {
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
            Value::Unresolved(path) => Some(Value::Unresolved(Class::new(
                &format!("{}.{}", path.module, path.name),
                attr,
            ))),
            Value::Instance(class) | Value::Type(class) => {
                for owner in self.order(class)?.iter() {
                    // A base that is not read may declare the attribute.
                    let facts = self.facts(&owner.module)?;
                    let found = facts.classes.get(&owner.name)?;
                    let env = Env {
                        this: Some(class),
                        ..Env::top(&owner.module)
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
            Value::Unresolved(path) => match path.form() {
                Some(Ok(Form::Cast)) => self.declared(cast?, env, depth + 1),
                Some(_) => None,
                None => Some(Value::Instance(path.clone())),
            },
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
                    this: receiver.as_ref(),
                    ..Env::top(module)
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
                self.call(&method, None, Env::top(self.module), depth + 1)
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
            self.eval(&val, Env { at: None, ..env }, depth + 1)
        };
        let type_of = |of: &Ann| match self.declared(of, env, depth + 1)? {
            Value::Instance(class) => Some(Value::Type(class)),
            _ => None,
        };
        match declared {
            Ann::Name(name) => match named(name)? {
                Value::Type(class) => Some(Value::Instance(class)),
                Value::Unresolved(path) => match path.form() {
                    Some(Ok(Form::Receiver)) => env.this.cloned().map(Value::Instance),
                    Some(_) => None,
                    None => Some(Value::Instance(path)),
                },
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
                let head = match named(head) {
                    Some(head) => head,
                    None if head == "type" => return type_of(args.first()?),
                    None => return None,
                };
                match head {
                    // The arguments of a generic class are not followed.
                    Value::Type(class) => Some(Value::Instance(class)),
                    Value::Unresolved(path) => match path.form() {
                        Some(Ok(Form::Transparent)) => self.declared(args.first()?, env, depth + 1),
                        Some(Ok(Form::Union)) => {
                            self.declared(&Ann::Union(args.clone()), env, depth + 1)
                        }
                        Some(Ok(Form::TypeOf)) => type_of(args.first()?),
                        Some(Ok(Form::Manager)) => {
                            let inner = self.declared(args.first()?, env, depth + 1)?;
                            Some(Value::Manager(Box::new(inner)))
                        }
                        Some(_) => None,
                        None => Some(Value::Instance(path)),
                    },
                    _ => None,
                }
            }
            Ann::None | Ann::Unknown => None,
        }
    }

    /// The method resolution order of `class`: itself, then its bases as Python's C3
    /// linearization orders them. A base that is not known stands in it as a class of
    /// no module, which no lookup goes past. `None` when the bases have no consistent
    /// order, or inherit from each other in a cycle.
    fn order(&self, class: &Class) -> Option<Rc<Vec<Class>>> {
        if let Some(known) = self.orders.borrow().get(class) {
            return known.clone();
        }
        // A class met again while its own order is worked out inherits from itself.
        self.orders.borrow_mut().insert(class.clone(), None);
        let order = self.linearized(class).map(Rc::new);
        self.orders
            .borrow_mut()
            .insert(class.clone(), order.clone());
        order
    }

    fn linearized(&self, class: &Class) -> Option<Vec<Class>> {
        let facts = self.facts(&class.module);
        let Some(known) = facts
            .as_ref()
            .and_then(|facts| facts.classes.get(&class.name))
        else {
            return Some(vec![class.clone()]);
        };
        let mut direct: Vec<Class> = Vec::new();
        let mut orders: Vec<Vec<Class>> = Vec::new();
        for base in &known.bases {
            let (base, order) = match self.eval(base, Env::top(&class.module), 1) {
                Some(Value::Type(base)) => {
                    let order = self.order(&base)?;
                    (base, (*order).clone())
                }
                Some(Value::Unresolved(base)) => (base.clone(), vec![base]),
                _ => {
                    let unknown = Class::new(NOWHERE, "");
                    (unknown.clone(), vec![unknown])
                }
            };
            if direct.contains(&base) {
                return None;
            }
            direct.push(base);
            orders.push(order);
        }
        orders.push(direct);
        let mut merged = vec![class.clone()];
        merged.extend(c3_merge(orders)?);
        Some(merged)
    }
}

/// The C3 merge of the orders of the bases and of the list of the bases: each time, the
/// first head that is in the tail of no order. `None` when no head qualifies.
fn c3_merge(mut orders: Vec<Vec<Class>>) -> Option<Vec<Class>> {
    let mut merged = Vec::new();
    loop {
        orders.retain(|order| !order.is_empty());
        if orders.is_empty() {
            return Some(merged);
        }
        if merged.len() > 256 {
            return None;
        }
        let head = orders
            .iter()
            .map(|order| &order[0])
            .find(|head| orders.iter().all(|order| !order[1..].contains(head)))?;
        let head = head.clone();
        for order in &mut orders {
            if order[0] == head {
                order.remove(0);
            }
        }
        merged.push(head);
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn class(name: &str) -> Class {
        Class::new("m", name)
    }

    fn order(orders: &[&[&str]]) -> Option<Vec<String>> {
        let orders = orders
            .iter()
            .map(|order| order.iter().map(|name| class(name)).collect())
            .collect();
        c3_merge(orders).map(|merged| merged.into_iter().map(|class| class.name).collect())
    }

    #[test]
    fn a_diamond_is_ordered_as_python_orders_it() {
        // class D(B, C), B(A), C(A): the orders of the bases, then the bases.
        let merged = order(&[&["B", "A"], &["C", "A"], &["B", "C"]]);
        assert_eq!(merged.unwrap(), ["B", "C", "A"]);
    }

    #[test]
    fn bases_in_no_consistent_order_have_no_order() {
        // class X(A, B), class Y(B, A), class Z(X, Y): TypeError in Python.
        assert_eq!(
            order(&[&["X", "A", "B"], &["Y", "B", "A"], &["X", "Y"]]),
            None
        );
        // class W(A, B) where B(A): A is before its subclass.
        assert_eq!(order(&[&["A"], &["B", "A"], &["A", "B"]]), None);
    }

    fn facts(text: &str) -> Facts {
        let source = Source::new(text, (3, 14)).expect("parses");
        Facts::of(&source, Some("pkg.m"))
    }

    #[test]
    fn every_binding_of_a_module_name_is_kept() {
        let found = facts(
            "import os\nfrom .db import Db as D\nclass C: ...\ndef f() -> C: ...\nx: C = f()\nx = None\nfor y in z: ...\n",
        );
        assert_eq!(found.names["os"], [Val::Module("os".to_string())]);
        let imported = Val::From {
            module: "pkg.db".to_string(),
            original: "Db".to_string(),
            local: "D".to_string(),
        };
        assert_eq!(found.names["D"], [imported]);
        assert_eq!(found.names["C"], [Val::Class("C".to_string())]);
        assert_eq!(found.names["f"], [Val::Function("f".to_string())]);
        let declared = Val::Declared(Ann::Name("C".to_string()));
        assert_eq!(found.names["x"], [declared, Val::None]);
        assert_eq!(found.names["y"], [Val::Unknown]);
        assert_eq!(found.functions["f"].returns, Ann::Name("C".to_string()));
    }

    #[test]
    fn a_name_bound_in_a_function_or_class_is_not_a_module_name() {
        let found = facts("def f():\n    import os\n    class Local: ...\n    x = 1\nclass C:\n    class Inner: ...\n");
        let mut names: Vec<&String> = found.names.keys().collect();
        names.sort();
        assert_eq!(names, ["C", "f"]);
        let mut classes: Vec<&String> = found.classes.keys().collect();
        classes.sort();
        assert_eq!(
            classes,
            ["C"],
            "a class in a class or function is not modelled"
        );
        assert_eq!(found.classes["C"].attrs["Inner"], [Val::Unknown]);
    }

    #[test]
    fn a_self_attribute_is_what_its_method_binds_it_to() {
        let found = facts(
            "class C:\n    kind: K\n    def __init__(self, db: Db, other):\n        self.db = db\n        self.other = other\n        self.made = make()\n        local = make()\n        self.local = local\n        self.me = self\n    def one(self): ...\n    one = None\n",
        );
        let attrs = &found.classes["C"].attrs;
        assert_eq!(attrs["kind"], [Val::Declared(Ann::Name("K".to_string()))]);
        assert_eq!(attrs["db"], [Val::Declared(Ann::Name("Db".to_string()))]);
        assert_eq!(
            attrs["other"],
            [Val::Unknown],
            "a parameter without annotation"
        );
        let made = Val::Call(Box::new(Val::Name("make".to_string())), None);
        assert_eq!(attrs["made"], [made]);
        assert_eq!(attrs["local"], [Val::Unknown], "a local is not followed");
        assert_eq!(attrs["me"], [Val::This]);
        assert_eq!(
            attrs["one"],
            [Val::None, Val::Unknown],
            "a method and an attribute"
        );
    }

    #[test]
    fn an_import_binds_the_first_part_of_a_dotted_module() {
        let found =
            facts("import a.b\nimport a.c as c\nfrom . import sibling\nfrom .. import up\n");
        assert_eq!(found.names["a"], [Val::Module("a".to_string())]);
        assert_eq!(found.names["c"], [Val::Module("a.c".to_string())]);
        let from = |module: &str, name: &str| Val::From {
            module: module.to_string(),
            original: name.to_string(),
            local: name.to_string(),
        };
        assert_eq!(found.names["sibling"], [from("pkg", "sibling")]);
        assert_eq!(found.names["up"], [from("", "up")]);
    }

    #[test]
    fn an_import_in_a_method_is_what_its_attribute_holds() {
        let found = facts(
            "class C:\n    def __init__(self):\n        from .db import Db\n        import lib\n        self.db = Db()\n        self.lib = lib\n",
        );
        let imported = Val::From {
            module: "pkg.db".to_string(),
            original: "Db".to_string(),
            local: "Db".to_string(),
        };
        let attrs = &found.classes["C"].attrs;
        assert_eq!(attrs["db"], [Val::Call(Box::new(imported), None)]);
        assert_eq!(attrs["lib"], [Val::Module("lib".to_string())]);
    }

    #[test]
    fn two_classes_of_one_name_are_unknown() {
        let found = facts("class C: ...\nclass C: ...\n");
        assert!(found.names["C"].contains(&Val::Unknown));
    }

    #[test]
    fn a_special_form_is_one_by_its_module() {
        assert_eq!(Class::new("typing", "cast").form(), Some(Ok(Form::Cast)));
        let extensions = Class::new("typing_extensions", "Self");
        assert_eq!(extensions.form(), Some(Ok(Form::Receiver)));
        assert_eq!(Class::new("typing", "Any").form(), Some(Err(())));
        assert_eq!(Class::new("lib", "cast").form(), None);
    }
}
