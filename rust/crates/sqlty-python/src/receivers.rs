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

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use ruff_python_ast::{Alias, Expr, Operator, Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::calls::{method_kind, Helper, Kind};
use crate::flow;
use crate::index::{imports_of, HelperIndex};
use crate::module::Import;
use crate::scopes::{dotted, Binding, Node, Origin, Scan, Scope};
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
    /// `Manager[T]` whose `with` gives `T`, `async with` when true.
    Manager(bool),
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
    ("typing", "ContextManager", Form::Manager(false)),
    ("typing", "AsyncContextManager", Form::Manager(true)),
    ("contextlib", "AbstractContextManager", Form::Manager(false)),
    (
        "contextlib",
        "AbstractAsyncContextManager",
        Form::Manager(true),
    ),
    (
        "contextlib",
        "_GeneratorContextManager",
        Form::Manager(false),
    ),
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
/// The statements of a scope whose shape is read at each use instead of kept.
const SHORT: usize = 4;
/// The module of the class that stands for a base that is not known.
const NOWHERE: &str = "\0";

/// The module of a class defined in a function or in another class of the checked
/// module, named by its scope: only the helpers it declares itself are known of it.
const NESTED: &str = "\0nested";

#[cfg(test)]
thread_local! {
    /// The steps the resolvers of this thread took: values evaluated and classes asked
    /// for a method.
    pub(crate) static STEPS: Cell<usize> = const { Cell::new(0) };
}

/// A class: the module that defines it and its name there. A class of a module that is not
/// read keeps the module and name it is imported by.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Class {
    pub module: Rc<str>,
    pub name: Rc<str>,
}

impl Class {
    fn new(module: &str, name: &str) -> Self {
        Class {
            module: module.into(),
            name: name.into(),
        }
    }

    /// What the name means when its module is one read by name; `Err` for a name of such
    /// a module that means nothing here.
    fn form(&self) -> Option<Result<Form, ()>> {
        let module = match &*self.module {
            "typing_extensions" => "typing",
            module => module,
        };
        KNOWN.contains(&module).then(|| {
            FORMS
                .iter()
                .find(|(owner, name, _)| *owner == module && **name == *self.name)
                .map(|(_, _, form)| *form)
                .ok_or(())
        })
    }
}

/// What an expression evaluates to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
        module: Rc<str>,
        class: Option<Rc<str>>,
        name: Rc<str>,
        receiver: Option<Class>,
    },
    /// A module.
    Module(String),
    /// A context manager whose `with` gives the value, `async with` when true.
    Manager(Box<Value>, bool),
    /// What calling an `async def` gives: the value once awaited, nothing before.
    Awaitable(Box<Value>),
    /// Any of the values, which differ: the bindings that reach a use, an import with a
    /// fallback, the members of a union. At most [`MAX_EITHER`], none of them `Either`.
    Either(Rc<[Value]>),
}

/// The most values a name is followed as at once: more leave it unknown, so that what is
/// read of each stays bounded.
const MAX_EITHER: usize = 8;

/// The one value that is any of `values`: itself when they are the same, `None` for
/// none or for more than are followed. With it, whether it is a new value of several.
fn either(values: impl IntoIterator<Item = Value>) -> Option<(Value, bool)> {
    let mut values = values.into_iter();
    let first = values.next()?;
    let mut found: Vec<Value> = Vec::new();
    let mut values = values.peekable();
    // The same value again, as the classes of one union give each other: itself.
    while values.next_if(|value| *value == first).is_some() {}
    if values.peek().is_none() {
        return Some((first, false));
    }
    for value in std::iter::once(first).chain(values) {
        match value {
            Value::Either(members) => {
                for member in members.iter() {
                    if !found.contains(member) {
                        found.push(member.clone());
                    }
                }
            }
            value if found.contains(&value) => {}
            value => found.push(value),
        }
        if found.len() > MAX_EITHER {
            return None;
        }
    }
    // The first and one that differs from it, or the members of an `Either`: several.
    Some((Value::Either(found.into()), true))
}

/// Whether a call reads its query, row type and parameters the same way through `one`
/// and through `other`: the helpers of two classes a receiver may be of.
fn same_helper(one: &Helper, other: &Helper) -> bool {
    std::ptr::eq(one, other)
        || (one.positional == other.positional
            && one.keywords == other.keywords
            && one.query == other.query
            && one.row == other.row
            && one.kind == other.kind
            && one.params == other.params
            && one.rest == other.rest
            && one.named == other.named
            && one.database == other.database)
}

/// A value that is any of several, the same as another when it is the same in memory:
/// the resolver keeps one of each, so that asking it again does not read its members.
#[derive(Clone)]
struct Set(Rc<[Value]>);

impl PartialEq for Set {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Set {}

impl std::hash::Hash for Set {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_ptr().hash(state);
    }
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

/// What is asked of bindings: a name of a scope of the checked module read in no order,
/// a name at the top of a module, the attribute a class declares, read on another, or a
/// name where some of its bindings reach.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Asked {
    Local(Scope, String),
    Member(Class, String, Class),
    /// What the function or method gives when called, bound to this receiver.
    Returned(Class, Option<Class>),
    /// What a value that is any of several gives when called, and what `with` binds
    /// when it is entered (`async with` when true).
    Called(Set),
    Entered(Set, bool),
}

/// The bindings of a name that reach a use, by where they are written.
type Sites = SmallVec<[u32; 4]>;

/// A name of a scope, by where the scan keeps its bindings, where the bindings written
/// at these places reach, under the `isinstance` test written at that one.
type Reached = (usize, Sites, Option<u32>);

/// What is worked out of a scope of the checked module, kept while the calls in it and
/// in the scopes inside it are read: the calls of a module come scope by scope, so that
/// what is kept at any time is what one nest of scopes takes.
#[derive(Default)]
struct Kept<'s> {
    /// The shape of its statements, whatever the name: walked once.
    shape: Option<Rc<flow::Shape<'s>>>,
    /// What each name holds through its statements, walked once, by where the scan
    /// keeps the bindings of the name; `None` for a scope without statements.
    flows: FxHashMap<usize, Option<Rc<flow::Flow<'s>>>>,
    /// What each lone declaration states, by where its annotation is written.
    stated: FxHashMap<usize, Option<Value>>,
    /// What a name holds where some of its bindings reach: asked once.
    reached: FxHashMap<Reached, Option<Value>>,
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
    /// A value of the annotated type: a parameter assigned to an attribute. Unlike a
    /// declaration, it does not state what else the attribute may hold.
    Typed(Ann),
    /// `with value as`, `async with` when true.
    Entered(Box<Val>, bool),
    /// `await value`.
    Awaited(Box<Val>),
    /// The class of this name, defined at the top of the module.
    Class(String),
    /// A class defined in a function or in another class, by where its name is written.
    Nested(u32),
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
    /// `async def`.
    pub asynchronous: bool,
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
    /// The bindings of each name, but for a name bound once, by a `def` that is kept in
    /// `functions`: [`Facts::bound`] gives those of any name.
    names: HashMap<String, Vec<Val>>,
}

impl Facts {
    /// What the top of the module binds `name` to, a value for each binding; `None`
    /// for a name it does not bind.
    pub fn bound(&self, name: &str) -> Option<Cow<'_, [Val]>> {
        match self.names.get(name) {
            Some(bound) => Some(Cow::Borrowed(bound)),
            None => self
                .functions
                .contains_key(name)
                .then(|| Cow::Owned(vec![Val::Function(name.to_string())])),
        }
    }

    /// Whether the top of the module binds `name`.
    pub fn binds(&self, name: &str) -> bool {
        self.names.contains_key(name) || self.functions.contains_key(name)
    }
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
        Expr::Name(name) => Val::Name(name.id.as_str().to_owned()),
        Expr::Attribute(attr) => Val::Attr(
            Box::new(value(&attr.value, depth + 1)),
            attr.attr.as_str().to_owned(),
        ),
        Expr::Call(call) => {
            let callee = value(&call.func, depth + 1);
            // `(await cast)(T, value)` names `cast` as `cast(T, value)` does.
            let mut named = &callee;
            while let Val::Awaited(awaited) = named {
                named = awaited;
            }
            let cast = match named {
                Val::Name(name) | Val::Attr(_, name) if name == "cast" => call
                    .arguments
                    .args
                    .first()
                    .map(|first| annotation(first, 0)),
                _ => None,
            };
            Val::Call(Box::new(callee), cast)
        }
        Expr::Await(awaited) => Val::Awaited(Box::new(value(&awaited.value, depth + 1))),
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
/// evaluate is written. A function is followed at the top of a module only, and of a
/// class defined elsewhere only the helpers it declares are (see [`Resolver::nested`]).
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
        Origin::Class(class) => (Val::Nested(class.name.start().to_u32()), None),
        Origin::Function(_) | Origin::Parameter | Origin::Unknown => (Val::Unknown, None),
    }
}

/// `ann` with the names `named` accepts unknown, the rest of it kept: `Box[T]` is
/// still a `Box`.
fn without(ann: Ann, named: &impl Fn(&str) -> bool) -> Ann {
    match ann {
        Ann::Name(name) if named(&name) => Ann::Unknown,
        Ann::Generic(head, _) if named(&head) => Ann::Unknown,
        Ann::Generic(head, args) => {
            let args = args.into_iter().map(|arg| without(arg, named));
            Ann::Generic(head, args.collect())
        }
        Ann::Union(members) => {
            let members = members.into_iter().map(|member| without(member, named));
            Ann::Union(members.collect())
        }
        ann => ann,
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
    // A type parameter of the function is a type of its own, whatever the module binds
    // the name to: it is unknown where a return annotation names it.
    let own = |name: &str| {
        let head = name.split('.').next().unwrap_or(name);
        let mut params = function.type_params.iter().flat_map(|params| params.iter());
        params.any(|param| crate::scopes::type_parameter(param).as_str() == head)
    };
    Method {
        returns: function.returns.as_deref().map_or(Ann::Unknown, |returns| {
            without(annotation(returns, 0), &own)
        }),
        kind,
        manager: decorated(function, &["contextmanager", "asynccontextmanager"]),
        asynchronous: function.is_async,
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
    match methods.entry(function.name.as_str().to_owned()) {
        Entry::Occupied(mut known) => {
            if known.get().returns != found.returns {
                known.get_mut().returns = Ann::Unknown;
            }
        }
        Entry::Vacant(new) => {
            new.insert(found);
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

/// `val` as the body of the class `scope` writes it: a name the body binds itself (an
/// alias, a nested class, another attribute) is not the module's, and is not followed.
/// `target = target`, bound once, reads the module's name: the body has not bound it yet.
fn in_class(val: Val, scan: &Scan<'_>, scope: Scope, target: &str) -> Val {
    let aim = Some(target);
    let own = |name: &str| class_binds(scan, scope, name, aim);
    match val {
        Val::Name(name) if own(&name) => Val::Unknown,
        Val::Attr(base, attr) => match in_class(*base, scan, scope, target) {
            Val::Unknown => Val::Unknown,
            base => Val::Attr(Box::new(base), attr),
        },
        Val::Call(callee, cast) => match in_class(*callee, scan, scope, target) {
            Val::Unknown => Val::Unknown,
            callee => {
                let cast = cast.map(|cast| match names_bound(&cast, scan, scope, aim) {
                    true => Ann::Unknown,
                    false => cast,
                });
                Val::Call(Box::new(callee), cast)
            }
        },
        Val::Entered(entered, is_async) => match in_class(*entered, scan, scope, target) {
            Val::Unknown => Val::Unknown,
            entered => Val::Entered(Box::new(entered), is_async),
        },
        Val::Awaited(awaited) => match in_class(*awaited, scan, scope, target) {
            Val::Unknown => Val::Unknown,
            awaited => Val::Awaited(Box::new(awaited)),
        },
        Val::Declared(declared) if names_bound(&declared, scan, scope, aim) => Val::Unknown,
        other => other,
    }
}

/// Whether the body of the class `scope` binds the head of `name` when it runs: `x: T`
/// alone declares and binds nothing. The one binding of `target` does not count, as its
/// own value and annotation are read before it.
fn class_binds(scan: &Scan<'_>, scope: Scope, name: &str, target: Option<&str>) -> bool {
    let head = name.split('.').next().unwrap_or(name);
    scan.bindings(scope, head).is_some_and(|bindings| {
        bindings
            .iter()
            .filter(|binding| !matches!(binding.origin, Origin::Declared(_, None)))
            .nth(usize::from(target == Some(head)))
            .is_some()
    })
}

/// Whether `name` is a type parameter of `class` (`class Box[T]`).
fn type_parameter(class: &StmtClassDef, name: &str) -> bool {
    let mut params = class.type_params.iter().flat_map(|params| params.iter());
    params.any(|param| crate::scopes::type_parameter(param).as_str() == name)
}

/// Whether `annotation` names something the body of the class `scope` binds.
fn names_bound(annotation: &Ann, scan: &Scan<'_>, scope: Scope, target: Option<&str>) -> bool {
    let own = |name: &str| class_binds(scan, scope, name, target);
    let inner = |annotation: &Ann| names_bound(annotation, scan, scope, target);
    match annotation {
        Ann::Name(name) => own(name),
        Ann::Generic(head, arguments) => own(head) || arguments.iter().any(inner),
        Ann::Union(members) => members.iter().any(inner),
        Ann::None | Ann::Unknown => false,
    }
}

/// Whether `binding` is a parameter of the method `scope` whose annotation names
/// something the body of its class binds: the annotation is read there, not in the module.
fn hidden_by_class(scan: &Scan<'_>, scope: Scope, binding: &Binding<'_>) -> bool {
    let (Node::Function(function), Origin::Declared(declared, None)) =
        (scan.scopes[scope], binding.origin)
    else {
        return false;
    };
    let class = scan.parents[scope];
    let parameter = function
        .body
        .first()
        .is_none_or(|first| binding.site < first.start().to_u32());
    parameter && scan.is_class(class) && names_bound(&annotation(declared, 0), scan, class, None)
}

/// `val` as the method `scope` of the module `module` writes it: its first parameter is
/// the receiver, a parameter it never binds again its annotation, a name it imports
/// what it imports, any other name it binds unknown.
fn inside(val: Val, scan: &Scan<'_>, scope: Scope, this: &str, module: Option<&str>) -> Val {
    match val {
        Val::Name(name) if name == this => Val::This,
        Val::Name(name) => match scan.bindings(scope, &name) {
            Some([only]) if hidden_by_class(scan, scope, only) => Val::Unknown,
            Some([only]) => match only.origin {
                Origin::Declared(declared, _) => Val::Typed(annotation(declared, 0)),
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
        Val::Awaited(awaited) => {
            Val::Awaited(Box::new(inside(*awaited, scan, scope, this, module)))
        }
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
                origin => {
                    let val = bound(origin, name, false, module).0;
                    vals.push(in_class(val, scan, scope, name));
                }
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
    // An annotation is read where the method is defined: in the body of the class.
    for method in facts.methods.values_mut() {
        if names_bound(&method.returns, scan, scope, None) {
            method.returns = Ann::Unknown;
        }
    }
    for (name, written) in scan.attributes_of(scope) {
        for (origin, method, function) in written {
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
        let scan = Scan::new(source.body());
        Self::from_scan(&scan, module)
    }

    /// What the module `scan` walked declares, its relative imports read as those of
    /// `module`.
    pub fn from_scan(scan: &Scan<'_>, module: Option<&str>) -> Facts {
        let mut facts = Facts::default();
        for (name, bindings) in scan.bound_in(0) {
            // A function defined once, as most are, is kept as a function only.
            if let [Binding {
                origin: Origin::Function(function),
                ..
            }] = bindings
            {
                let known = facts.functions.len();
                define(&mut facts.functions, function, Kind::Function);
                if facts.functions.len() > known {
                    continue;
                }
            }
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
    /// What is kept of the module and of the scopes around the call that is read.
    kept: RefCell<FxHashMap<Scope, Kept<'s>>>,
    /// The scopes around the call read last, outermost first.
    around: RefCell<Vec<Scope>>,
    /// The shape of statements that only run one after the other.
    plain: Rc<flow::Shape<'s>>,
    /// What the bindings of a name or attribute, read in no order, give it: asked once.
    agreed: RefCell<HashMap<Asked, Option<Value>>>,
    /// What each name is at the top of each module: asked once.
    globals: RefCell<HashMap<String, HashMap<String, Option<Value>>>>,
    /// What each attribute of a value that is any of several is: asked once.
    attrs: RefCell<FxHashMap<Set, HashMap<String, Option<Value>>>>,
    /// What a dotted name of an annotation is, read where no scope binds its head.
    spelled: RefCell<HashMap<String, Option<Value>>>,
    /// How many classes of the module have each name, counted once.
    named_classes: RefCell<Option<HashMap<String, usize>>>,
    /// The helper each method of each class is: asked once.
    methods: RefCell<HashMap<String, HashMap<Class, Option<&'s Helper>>>>,
    /// The helper a method of a value that is any of several classes calls: asked once.
    called: RefCell<HashMap<(Set, String), Option<&'s Helper>>>,
    /// The values that are any of several, one of each.
    sets: RefCell<HashSet<Rc<[Value]>>>,
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
            kept: RefCell::default(),
            around: RefCell::default(),
            plain: Rc::default(),
            agreed: RefCell::default(),
            named_classes: RefCell::default(),
            methods: RefCell::default(),
            globals: RefCell::default(),
            attrs: RefCell::default(),
            spelled: RefCell::default(),
            called: RefCell::default(),
            sets: RefCell::default(),
            fuel: Cell::new(0),
        }
    }

    /// The helper that `receiver.name(...)` calls in the scopes `stack`: the method `name`
    /// of the class of `receiver`, or of the first class of its method resolution order
    /// that defines it.
    ///
    /// A receiver that is of any of several classes calls the helper when each class
    /// reaches one and they take the call the same way.
    pub fn method(&self, receiver: &Expr, name: &str, stack: &[Scope]) -> Option<&'s Helper> {
        let value = self.value_of(receiver, stack)?;
        let Value::Either(members) = &value else {
            return self.method_of(&value, name);
        };
        // The classes are asked once for the method, whatever the number of calls.
        let key = (Set(Rc::clone(members)), name.to_string());
        if let Some(known) = self.called.borrow().get(&key) {
            return *known;
        }
        let found = self.method_of_each(members, name);
        self.called.borrow_mut().insert(key, found);
        found
    }

    fn method_of_each(&self, members: &[Value], name: &str) -> Option<&'s Helper> {
        let mut found: Option<&'s Helper> = None;
        for member in members {
            let helper = self.method_of(member, name)?;
            if found.is_some_and(|known| !same_helper(known, helper)) {
                return None;
            }
            found = found.or(Some(helper));
        }
        found
    }

    fn method_of(&self, receiver: &Value, name: &str) -> Option<&'s Helper> {
        #[cfg(test)]
        STEPS.with(|steps| steps.set(steps.get() + 1));
        let (class, on_class) = match receiver {
            Value::Instance(class) => (class, false),
            Value::Type(class) | Value::Unresolved(class) => (class, true),
            _ => return None,
        };
        // Asked at every call of the method: kept by the borrowed name and class.
        let known = self
            .methods
            .borrow()
            .get(name)
            .map(|classes| classes.get(class).copied());
        let found = match known {
            Some(Some(found)) => found,
            _ => {
                let found = self.method_in(class, name);
                let mut methods = self.methods.borrow_mut();
                let classes = methods.entry(name.to_string()).or_default();
                classes.insert(class.clone(), found);
                found
            }
        };
        found.filter(|found| !on_class || found.kind != Kind::Instance)
    }

    /// The class of the scope `scope`, defined in a function or in another class, when
    /// it is the only class of its name in the module and the scope around it binds
    /// that name once: nothing else can then be meant by the name, or own its helpers.
    fn nested(&self, scope: Scope) -> Option<Class> {
        let name = self.scan.class(scope)?.name.as_str();
        let around = self.scan.parents[scope];
        let once = matches!(
            self.scan.bindings(around, name)?,
            [Binding {
                origin: Origin::Class(_),
                ..
            }]
        );
        if !once || self.scan.shadows(around, name) {
            return None;
        }
        let mut named = self.named_classes.borrow_mut();
        let counts = named.get_or_insert_with(|| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for node in &self.scan.scopes {
                if let Node::Class(class) = node {
                    *counts.entry(class.name.to_string()).or_default() += 1;
                }
            }
            counts
        });
        (counts.get(name) == Some(&1)).then(|| Class::new(NESTED, &scope.to_string()))
    }

    /// The helper the method `name` of `class` is, on the class or on an instance.
    fn method_in(&self, class: &Class, name: &str) -> Option<&'s Helper> {
        // A class whose bases have no order still has the methods it declares itself.
        let order = self.order(class);
        let classes = order
            .as_deref()
            .map_or(std::slice::from_ref(class), Vec::as_slice);
        for base in classes {
            if &*base.module == NESTED {
                // The helpers of the module are kept by the name of their class, which
                // no other class of the module has (see `nested`).
                let class = self.scan.class(base.name.parse().ok()?)?;
                return self.own.get(&format!("{}.{name}", class.name));
            }
            let key = format!("{}.{name}", base.name);
            let own = (&*base.module == self.module)
                .then(|| self.own.get(&key))
                .flatten();
            // A class its module defines has the helpers of that module; a name known
            // by its path only may be one the module re-exports.
            let found = own.or_else(|| {
                let helpers = self.helpers?;
                let defined = self
                    .facts(&base.module)
                    .is_some_and(|facts| facts.classes.contains_key(&*base.name));
                match defined {
                    true => helpers.modules.get(&*base.module)?.get(&key),
                    false => helpers.get(&base.module, &key),
                }
            });
            if found.is_some() {
                return found;
            }
            // A base that is not read may define the method, and a class that defines it
            // itself hides the helper of its bases.
            let facts = self.facts(&base.module)?;
            let known = facts.classes.get(&*base.name)?;
            if known.methods.contains_key(name) || known.attrs.contains_key(name) {
                return None;
            }
        }
        None
    }

    /// Forgets what was kept of the scopes that are not around a call in `stack`: no
    /// later call is in them, as the calls of a scope and of those inside it come
    /// together.
    fn around(&self, stack: &[Scope]) {
        let mut around = self.around.borrow_mut();
        if around[..] == *stack {
            return;
        }
        let same = around.iter().zip(stack).take_while(|(a, b)| a == b).count();
        let mut kept = self.kept.borrow_mut();
        for left in around.drain(same..) {
            kept.remove(&left);
        }
        around.extend_from_slice(&stack[same..]);
    }

    fn value_of(&self, receiver: &Expr, stack: &[Scope]) -> Option<Value> {
        self.around(stack);
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
        if KNOWN.contains(&module) || module.is_empty() || module == NOWHERE || module == NESTED {
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

    /// The one value that is any of `values`, as [`either`] gives it, kept once.
    fn any(&self, values: impl IntoIterator<Item = Value>) -> Option<Value> {
        let (found, new) = either(values)?;
        let (Value::Either(members), true) = (&found, new) else {
            return Some(found);
        };
        if let Some(kept) = self.sets.borrow().get(&members[..]) {
            return Some(Value::Either(Rc::clone(kept)));
        }
        self.sets.borrow_mut().insert(Rc::clone(members));
        Some(found)
    }

    /// What `asked` is, worked out once with a budget of its own, so that the answer
    /// does not depend on who asks first. A question that asks itself has no answer.
    fn once(&self, asked: Asked, answer: impl FnOnce() -> Option<Value>) -> Option<Value> {
        self.once_in(&self.agreed, asked, answer)
    }

    fn once_in<K: Clone + Eq + std::hash::Hash, S: std::hash::BuildHasher>(
        &self,
        known: &RefCell<HashMap<K, Option<Value>, S>>,
        asked: K,
        answer: impl FnOnce() -> Option<Value>,
    ) -> Option<Value> {
        if let Some(known) = known.borrow().get(&asked) {
            return known.clone();
        }
        known.borrow_mut().insert(asked.clone(), None);
        let left = self.fuel.replace(FUEL);
        let found = answer();
        self.fuel.set(left);
        known.borrow_mut().insert(asked, found.clone());
        found
    }

    /// Whether another step may be taken at `depth`.
    fn step(&self, depth: u8) -> Option<()> {
        #[cfg(test)]
        STEPS.with(|steps| steps.set(steps.get() + 1));
        let left = self.fuel.get().checked_sub(1)?;
        self.fuel.set(left);
        (depth < DEPTH).then_some(())
    }

    #[recursive::recursive]
    fn eval(&self, val: &Val, env: Env<'_>, depth: u8) -> Option<Value> {
        self.step(depth)?;
        match val {
            Val::Name(name) => match self.local_name(name, env) {
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
            Val::Declared(declared) | Val::Typed(declared) => {
                self.declared(declared, env, depth + 1)
            }
            Val::Entered(entered, is_async) => {
                let entered = self.eval(entered, env, depth + 1)?;
                self.enter(&entered, *is_async, depth + 1)
            }
            Val::Awaited(awaited) => {
                let awaited = self.eval(awaited, env, depth + 1)?;
                self.awaited(awaited)
            }
            Val::Class(name) => Some(Value::Type(Class::new(env.module, name))),
            Val::Nested(site) => {
                let local = env.module == self.module;
                let scope = self.scan.class_at(*site).filter(|_| local)?;
                self.nested(scope).map(Value::Type)
            }
            Val::Function(name) => Some(Value::Function {
                module: env.module.into(),
                class: None,
                name: name.as_str().into(),
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
        let mut found: SmallVec<[Value; 2]> = SmallVec::new();
        let mut last: Option<&Val> = None;
        // What was read, and whether it gives the same wherever it is read, which is
        // asked when it is met again: the first few that do, or are not known not to.
        let mut read: SmallVec<[(&Val, Option<bool>); MAX_EITHER]> = SmallVec::new();
        for (val, at) in bound {
            if matches!(val, Val::None) {
                continue;
            }
            // The same value read at no place gives what it gave: a name bound to it
            // any number of times costs one reading.
            let mut again = at.is_none() && last == Some(val);
            let known = read.iter().position(|(known, _)| *known == val);
            if let (false, Some(known)) = (again, known) {
                let placeless = &mut read[known].1;
                again = *placeless.get_or_insert_with(|| self.placeless(val, env));
            }
            if again {
                continue;
            }
            last = Some(val);
            let value = self.eval(val, Env { at: *at, ..env }, depth)?;
            if known.is_none() {
                if read.len() == MAX_EITHER {
                    read.retain(|(known, placeless)| {
                        *placeless.get_or_insert_with(|| self.placeless(known, env))
                    });
                }
                if read.len() < MAX_EITHER {
                    read.push((val, None));
                }
            }
            if !found.contains(&value) {
                found.push(value);
            }
        }
        // Bindings that differ: the name is any of what they give it.
        self.any(found)
    }

    /// Whether `val` gives the same wherever it is read in the scopes of `env`: none of
    /// them binds a name of it, in its statements or in a comprehension or lambda.
    fn placeless(&self, val: &Val, env: Env<'_>) -> bool {
        match val {
            Val::Name(name) => {
                // A name of the module is read in no order from a function only.
                let local = env.module == self.module;
                let function = env.stack.iter().any(|&scope| !self.scan.is_class(scope));
                let bound = |&scope: &Scope| {
                    self.scan.bindings(scope, name).is_some() || self.scan.shadows(scope, name)
                };
                !local || (function && !env.stack.iter().any(bound) && !self.scan.shadows(0, name))
            }
            Val::Attr(base, _)
            | Val::Entered(base, _)
            | Val::Awaited(base)
            | Val::Call(base, None) => self.placeless(base, env),
            Val::Call(_, Some(_)) => false,
            _ => true,
        }
    }

    /// What the bindings of a name or attribute, read in no order, give it.
    fn decl(&self, bound: &[Val], env: Env<'_>, depth: u8) -> Option<Value> {
        let bound: Vec<(Val, Option<u32>)> = bound.iter().map(|val| (val.clone(), None)).collect();
        self.agreed(&bound, true, env, depth)
    }

    /// What `name` is bound to by a scope around the expression; `None` when none of them
    /// binds it, or when it is a name of the module read from a function: then it is what
    /// the module binds it to, in no order.
    fn local_name(&self, name: &str, env: Env<'_>) -> Option<Option<Value>> {
        if !self.own(env.module) {
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
                // A lambda or a comprehension in the body of a class does not see the
                // names of the class, as a method does not.
                Node::Class(class)
                    if scope == innermost
                        && !env.at.is_some_and(|at| self.scan.sealed(scope, at)) =>
                {
                    &class.body
                }
                // A type parameter of a class is seen in its methods, unlike the names
                // its body binds.
                Node::Class(class) if type_parameter(class, name) => return Some(None),
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
                    // Of a class in a function or in another class, only the helpers
                    // it declares are known.
                    let class = match top {
                        true => Some(Class::new(self.module, class.name.as_str())),
                        false => outer.and_then(|outer| self.nested(outer)),
                    };
                    let on_class = method_kind(function) == Kind::Class
                        || matches!(function.name.as_str(), "__new__" | "__init_subclass__");
                    return Some(class.map(|class| match on_class {
                        true => Value::Type(class),
                        false => Value::Instance(class),
                    }));
                }
            }
            let Some(bindings) = bindings else {
                continue;
            };
            // Only a parameter is hidden, and a parameter is the first binding of its name.
            let hidden = bindings
                .first()
                .is_some_and(|first| hidden_by_class(self.scan, scope, first));
            let id = bindings.as_ptr().addr();
            // What a hidden parameter gives the name is unknown.
            let origin = |index: usize, binding: &Binding<'s>| match hidden && index == 0 {
                true => Origin::Unknown,
                false => binding.origin,
            };
            let reach = env
                .at
                .filter(|_| scope == innermost)
                .and_then(|at| self.held((scope, id), name, body, bindings, hidden, at));
            let this = class.map(|(class, _)| Class::new(self.module, class.name.as_str()));
            let env = Env {
                stack: &env.stack[..=i],
                this: this.as_ref(),
                ..env
            };
            return Some(match reach {
                Some(reach) => self.reached((name, id), &reach, false, env),
                None => self.once(Asked::Local(scope, name.to_string()), || {
                    let bound: Vec<_> = bindings
                        .iter()
                        .enumerate()
                        .map(|(index, binding)| {
                            bound(origin(index, binding), name, false, self.named)
                        })
                        .collect();
                    self.agreed(&bound, true, env, 0)
                }),
            });
        }
        // A function reads the names of the module when it is called, whatever the
        // order of its statements: so does the body of a lambda in a class.
        let later = env
            .at
            .is_some_and(|at| self.scan.called_later(innermost, at));
        if later || env.stack.iter().any(|&scope| !self.scan.is_class(scope)) {
            return None;
        }
        let bindings = self.scan.bindings(0, name)?;
        let id = bindings.as_ptr().addr();
        let body = self.source.body();
        let reach = self.held((0, id), name, body, bindings, false, env.at?)?;
        // A name of the module is what the module makes it, whatever the class it is
        // read in: `Self` there names no class.
        let env = Env {
            stack: &[],
            this: None,
            ..env
        };
        Some(self.reached((name, id), &reach, true, env))
    }

    /// What `name` holds at `at` in `body`, the statements of `scope`, followed in the
    /// order they run: the scope is walked once for the name, whatever the number of
    /// uses. `None` for a use in a lambda or a generator that may run after the name is
    /// bound again: a binding of the scope takes effect after that code starts, or is in
    /// a loop around it. Every binding of the scope then counts, as for a nested function.
    fn held(
        &self,
        key: (Scope, usize),
        name: &str,
        body: &'s [Stmt],
        bindings: &'s [Binding<'s>],
        hidden: bool,
        at: u32,
    ) -> Option<flow::Reach<'s>> {
        let (scope, id) = key;
        let known = {
            let kept = self.kept.borrow();
            let kept = kept.get(&scope);
            let flow = kept.and_then(|kept| kept.flows.get(&id).cloned());
            flow.ok_or_else(|| kept.and_then(|kept| kept.shape.clone()))
        };
        let flow = match known {
            Ok(flow) => flow,
            Err(shape) => {
                let shape = shape.unwrap_or_else(|| {
                    // Most scopes are short and plain: they share one shape, which is
                    // not kept for the shortest, read again in less than it takes.
                    let plain = flow::Shape::plain(body);
                    if plain && body.len() <= SHORT {
                        return Rc::clone(&self.plain);
                    }
                    let shape = match plain {
                        true => Rc::clone(&self.plain),
                        false => Rc::new(flow::Shape::new(body)),
                    };
                    let mut kept = self.kept.borrow_mut();
                    kept.entry(scope).or_default().shape = Some(Rc::clone(&shape));
                    shape
                });
                // A parameter is hidden as a class of its name hides it.
                let origin = |binding: &Binding<'s>| match hidden {
                    true => Origin::Unknown,
                    false => binding.origin,
                };
                if let Some(reach) = self.bound_once(scope, name, body, bindings, &shape, at) {
                    return reach.map(|site| flow::Reach {
                        origins: site
                            .then(|| (origin(&bindings[0]), bindings[0].site))
                            .into_iter()
                            .collect(),
                        narrowed: None,
                    });
                }
                let flow = flow::Flow::new(body, shape, bindings, hidden, name).map(Rc::new);
                let mut kept = self.kept.borrow_mut();
                let flows = &mut kept.entry(scope).or_default().flows;
                flows.insert(id, flow.clone());
                flow
            }
        };
        let flow = flow?;
        if let Some(start) = self.scan.deferred(scope, at) {
            if flow.binds_after(start, flow.loop_around(at)) {
                return None;
            }
        }
        flow.at(at)
    }

    /// What [`Self::held`] gives for a name the scope binds once, as a parameter or by
    /// a statement of its own in `body`, and never tests: it holds that binding at
    /// every use after it, so the statements are not walked for it. `None` for another
    /// name; else whether the binding reaches `at`, or `None` as `held` gives it.
    fn bound_once(
        &self,
        scope: Scope,
        name: &str,
        body: &[Stmt],
        bindings: &[Binding<'_>],
        shape: &flow::Shape<'_>,
        at: u32,
    ) -> Option<Option<bool>> {
        let ([only], false) = (bindings, shape.tests(name)) else {
            return None;
        };
        let start = body.first()?.start().to_u32();
        let parameter = only.site < start && only.effect <= start;
        if !parameter {
            let next = body.partition_point(|stmt| stmt.end().to_u32() <= only.site);
            let stmt = body.get(next)?;
            let plain = matches!(
                stmt,
                Stmt::Assign(_) | Stmt::AnnAssign(_) | Stmt::Import(_) | Stmt::ImportFrom(_)
            );
            let within = stmt.start().to_u32() <= only.site && only.effect <= stmt.end().to_u32();
            if !plain || !within {
                return None;
            }
        }
        // Code that runs later may run after a binding written from there on.
        if let Some(later) = self.scan.deferred(scope, at) {
            if only.site >= later || only.effect > later {
                return Some(None);
            }
        }
        Some(Some(only.effect <= at && !shape.unreached(at)))
    }

    /// What `name` holds where the bindings `reach` are in effect. An `isinstance` test
    /// states the class of a name nothing else is known of (no annotation, `object`,
    /// `Any`, a value that is not followed), and of one known to be of a base of that
    /// class; a subclass of the class stays what it is. A name known to be of an
    /// unrelated class would be of both, which is not followed.
    fn reached(
        &self,
        (name, id): (&str, usize),
        reach: &flow::Reach<'_>,
        top: bool,
        env: Env<'_>,
    ) -> Option<Value> {
        // The uses the same bindings reach, under the same test, hold the same.
        let unknown = matches!(reach.origins[..], [(Origin::Unknown, _)]);
        if unknown && reach.narrowed.is_none() {
            return None;
        }
        // A declaration alone states the type, read where no statement is: it asks
        // nothing of the names of the scope, and is kept by where it is written.
        let scope = env.stack.last().copied().unwrap_or(0);
        if let ([(Origin::Declared(declared, _), _)], None) = (&reach.origins[..], reach.narrowed) {
            // A name the module makes a class is kept by how it is spelled.
            if let Expr::Name(spelled) = declared {
                let name = spelled.id.as_str();
                let known = self.spelled.borrow().get(name).cloned();
                if let (Some(Some(Value::Type(class))), true) = (known, self.of_module(name, env)) {
                    return Some(Value::Instance(class));
                }
            }
            let written = std::ptr::from_ref::<Expr>(declared).addr();
            let known = |kept: &Kept<'_>| kept.stated.get(&written).cloned();
            if let Some(known) = self.kept.borrow().get(&scope).and_then(known) {
                return known;
            }
            let left = self.fuel.replace(FUEL);
            let found = self.declared(&annotation(declared, 0), Env { at: None, ..env }, 1);
            self.fuel.set(left);
            let mut kept = self.kept.borrow_mut();
            let stated = &mut kept.entry(scope).or_default().stated;
            stated.insert(written, found.clone());
            return found;
        }
        let sites: Sites = reach.origins.iter().map(|(_, site)| *site).collect();
        let test = reach.narrowed.map(|test| test.start().to_u32());
        let asked = (id, sites, test);
        let known = |kept: &Kept<'_>| kept.reached.get(&asked).cloned();
        if let Some(known) = self.kept.borrow().get(&scope).and_then(known) {
            return known;
        }
        // A name that asks for itself has no answer; the answer has a budget of its
        // own, so that it does not depend on who asks first.
        let keep = |found: Option<Value>| {
            let mut kept = self.kept.borrow_mut();
            let reached = &mut kept.entry(scope).or_default().reached;
            reached.insert(asked.clone(), found.clone());
            found
        };
        keep(None);
        let left = self.fuel.replace(FUEL);
        let found = self.reached_now(name, reach, top, env, 0);
        self.fuel.set(left);
        keep(found)
    }

    fn reached_now(
        &self,
        name: &str,
        reach: &flow::Reach<'_>,
        top: bool,
        env: Env<'_>,
        depth: u8,
    ) -> Option<Value> {
        let bound: SmallVec<[(Val, Option<u32>); 2]> = reach
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
        let mut held: Vec<Class> = Vec::new();
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
                Some(value @ (Value::Instance(_) | Value::Either(_))) => {
                    let members = match &value {
                        Value::Either(members) => &members[..],
                        value => std::slice::from_ref(value),
                    };
                    for member in members {
                        let Value::Instance(known) = member else {
                            return None;
                        };
                        if !held.contains(known) {
                            held.push(known.clone());
                        }
                    }
                    if held.len() > MAX_EITHER {
                        return None;
                    }
                }
                None if !spelled && !matches!(val, Val::None) => unknown = true,
                _ => return None,
            }
        }
        if held.is_empty() {
            return Some(Value::Instance(class));
        }
        if unknown {
            return None;
        }
        // The test keeps the classes that are the tested class or a subclass of it; a
        // name that may be of a base of the tested class may be of the tested one.
        let mut kept = Vec::new();
        let mut base = false;
        for known in held {
            if self.order(&known)?.contains(&class) {
                kept.push(Value::Instance(known));
            } else if self.order(&class)?.contains(&known) {
                base = true;
            }
        }
        if base {
            kept.push(Value::Instance(class));
        }
        self.any(kept)
    }

    /// What `name` is at the top of `module`, whatever the order of its statements. A
    /// module that is not read gives the name by its path.
    #[recursive::recursive]
    fn global(&self, module: &str, name: &str, depth: u8) -> Option<Value> {
        self.step(depth)?;
        let Some(facts) = self.facts(module) else {
            return Some(Value::Unresolved(Class::new(module, name)));
        };
        let bound = facts.bound(name)?;
        // Asked at every use of the name: the answer is looked up by the borrowed names.
        let known = self
            .globals
            .borrow()
            .get(module)
            .map(|names| names.get(name).cloned());
        match known {
            Some(Some(known)) => return known,
            Some(None) => {}
            None => {
                let mut globals = self.globals.borrow_mut();
                globals.insert(module.to_string(), HashMap::new());
            }
        }
        let keep = |found: Option<Value>| {
            let mut globals = self.globals.borrow_mut();
            let names = globals.get_mut(module)?;
            names.insert(name.to_string(), found.clone());
            found
        };
        // A name that asks for itself has no answer.
        keep(None);
        let left = self.fuel.replace(FUEL);
        let found = self.decl(&bound, Env::top(module), 0);
        self.fuel.set(left);
        keep(found)
    }

    /// `found`, what the name `name` of the module `module` was read to be, unless a
    /// method of `module.name` is registered and the name was not read to be that class:
    /// the module gets it from a C extension, a star import, `__getattr__` or an import
    /// with a fallback. It is then known by its path, as when the module is not found,
    /// so that what is registered applies to it.
    fn or_registered(&self, module: &str, name: &str, found: Option<Value>) -> Option<Value> {
        let registers = |module: &str, name: &str| {
            self.helpers
                .is_some_and(|helpers| helpers.registers(module, name))
        };
        let read_as = |value: &Value| match value {
            Value::Type(class) => {
                (&*class.module == module && &*class.name == name)
                    || registers(&class.module, &class.name)
            }
            Value::Unresolved(class) => &*class.module == module && &*class.name == name,
            _ => true,
        };
        let read = match &found {
            // Any of several: each has to be a class the registration applies to.
            Some(Value::Either(members)) => members.iter().all(read_as),
            Some(value) => read_as(value),
            None => false,
        };
        match read || !registers(module, name) {
            true => found,
            false => Some(Value::Unresolved(Class::new(module, name))),
        }
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
        let found = self.imported_as_read(module, target, original, local, depth);
        self.or_registered(target, original, found)
    }

    fn imported_as_read(
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
            Some(found) if !itself && found.binds(original) => {
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
                if found.as_ref().is_some_and(|facts| facts.binds(attr)) {
                    let read = self.global(module, attr, depth + 1);
                    return self.or_registered(module, attr, read);
                }
                // A package does not have to import its submodule for `import a.b` to
                // bind it.
                let submodule = format!("{module}.{attr}");
                if self.facts(&submodule).is_some() {
                    Some(Value::Module(submodule))
                } else if found.is_none() {
                    self.global(module, attr, depth + 1)
                } else {
                    self.or_registered(module, attr, None)
                }
            }
            Value::Unresolved(path) => Some(Value::Unresolved(Class::new(
                &format!("{}.{}", path.module, path.name),
                attr,
            ))),
            Value::Type(class) => {
                // A class in a class is not modelled: a method registered on it is
                // found by its path.
                let read = self.member_of(class, attr, depth);
                let outer = format!("{}.{}", class.module, class.name);
                self.or_registered(&outer, attr, read)
            }
            Value::Instance(class) => self.member_of(class, attr, depth),
            // The attribute of any of them: each has to have it.
            Value::Either(members) => {
                // Asked at every use of the attribute: looked up by the borrowed name.
                let set = Set(Rc::clone(members));
                let known = self
                    .attrs
                    .borrow()
                    .get(&set)
                    .map(|attrs| attrs.get(attr).cloned());
                match known {
                    Some(Some(known)) => return known,
                    Some(None) => {}
                    None => {
                        self.attrs.borrow_mut().insert(set.clone(), HashMap::new());
                    }
                }
                let keep = |found: Option<Value>| {
                    let mut attrs = self.attrs.borrow_mut();
                    attrs.get_mut(&set)?.insert(attr.to_string(), found.clone());
                    found
                };
                // An attribute that asks for itself has no answer.
                keep(None);
                let left = self.fuel.replace(FUEL);
                let found: Option<Vec<Value>> = members
                    .iter()
                    .map(|member| self.member(member, attr, 0))
                    .collect();
                let found = found.and_then(|found| self.any(found));
                self.fuel.set(left);
                keep(found)
            }
            Value::Function { .. } | Value::Manager(..) | Value::Awaitable(_) => None,
        }
    }

    /// The attribute `attr` of the class `class` or of an instance of it.
    fn member_of(&self, class: &Class, attr: &str, depth: u8) -> Option<Value> {
        {
            {
                for owner in self.order(class)?.iter() {
                    // A base that is not read may declare the attribute.
                    let facts = self.facts(&owner.module)?;
                    let found = facts.classes.get(&*owner.name)?;
                    let env = Env {
                        this: Some(class),
                        ..Env::top(&owner.module)
                    };
                    if let Some(bound) = found.attrs.get(attr) {
                        let asked = Asked::Member(owner.clone(), attr.to_string(), class.clone());
                        return self.once(asked, || self.decl(bound, env, 0));
                    }
                    if let Some(method) = found.methods.get(attr) {
                        if method.property {
                            return self.declared(&method.returns, env, depth + 1);
                        }
                        return Some(Value::Function {
                            module: owner.module.clone(),
                            class: Some(owner.name.clone()),
                            name: attr.into(),
                            receiver: Some(class.clone()),
                        });
                    }
                }
                None
            }
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
            // What any of them gives when called: each has to be followed.
            // Without a type to cast to, it does not depend on where: asked once.
            Value::Either(members) => {
                let each = |depth: u8| {
                    let found: Option<Vec<Value>> = members
                        .iter()
                        .map(|member| self.call(member, cast, env, depth))
                        .collect();
                    self.any(found?)
                };
                match cast {
                    Some(_) => each(depth + 1),
                    None => self.once(Asked::Called(Set(Rc::clone(members))), || each(0)),
                }
            }
            Value::Function {
                module,
                class,
                name,
                receiver,
            } => {
                let facts = self.facts(module)?;
                let method = match class {
                    Some(class) => facts.classes.get(&**class)?.methods.get(&**name)?,
                    None => facts.functions.get(&**name)?,
                };
                let env = Env {
                    this: receiver.as_ref(),
                    ..Env::top(module)
                };
                let owner = Class::new(
                    module,
                    &match class {
                        Some(class) => format!("{class}.{name}"),
                        None => name.to_string(),
                    },
                );
                let asked = Asked::Returned(owner, receiver.clone());
                self.once(asked, || self.returned(method, env))
            }
            _ => None,
        }
    }

    /// What calling `method` gives, its annotation read in `env`.
    fn returned(&self, method: &Method, env: Env<'_>) -> Option<Value> {
        if method.manager {
            let Ann::Generic(head, args) = &method.returns else {
                return None;
            };
            YIELDING
                .contains(&head.rsplit('.').next().unwrap_or(head))
                .then_some(())?;
            let yielded = self.declared(args.first()?, env, 0)?;
            return Some(Value::Manager(Box::new(yielded), method.asynchronous));
        }
        let returned = self.declared(&method.returns, env, 0)?;
        Some(match method.asynchronous {
            true => Value::Awaitable(Box::new(returned)),
            false => returned,
        })
    }

    /// What `await awaited` gives: what an `async def` returns, and anything else as
    /// it is, an awaitable of another kind being annotated with what it gives.
    fn awaited(&self, awaited: Value) -> Option<Value> {
        match awaited {
            Value::Awaitable(inner) => Some(*inner),
            Value::Either(members) => {
                let found = members.iter().cloned().map(|member| match member {
                    Value::Awaitable(inner) => *inner,
                    member => member,
                });
                self.any(found.collect::<Vec<Value>>())
            }
            awaited => Some(awaited),
        }
    }

    /// What `with entered as` binds.
    fn enter(&self, entered: &Value, is_async: bool, depth: u8) -> Option<Value> {
        match entered {
            // `with` enters one kind of manager, `async with` the other.
            Value::Manager(inner, entered) => (*entered == is_async).then(|| (**inner).clone()),
            Value::Either(members) => {
                self.once(Asked::Entered(Set(Rc::clone(members)), is_async), || {
                    let found: Option<Vec<Value>> = members
                        .iter()
                        .map(|member| self.enter(member, is_async, 0))
                        .collect();
                    self.any(found?)
                })
            }
            Value::Instance(_) => {
                let name = if is_async { "__aenter__" } else { "__enter__" };
                let method = self.member(entered, name, depth + 1)?;
                let given = self.call(&method, None, Env::top(self.module), depth + 1)?;
                // `async with` awaits what `__aenter__` gives.
                match is_async {
                    true => self.awaited(given),
                    false => Some(given),
                }
            }
            _ => None,
        }
    }

    /// Whether `head`, the first name of an annotation of the checked module, is the
    /// module's: no scope around binds it, or has it as a type parameter.
    fn of_module(&self, head: &str, env: Env<'_>) -> bool {
        let bound = |&scope: &Scope| {
            let parameter = matches!(self.scan.scopes[scope], Node::Class(class) if type_parameter(class, head));
            parameter || self.scan.bindings(scope, head).is_some()
        };
        self.own(env.module) && !env.stack.iter().any(bound)
    }

    /// Whether `module` is the module being checked. Its name is most often the very
    /// string the resolver holds, which needs no comparison.
    fn own(&self, module: &str) -> bool {
        std::ptr::eq(module, self.module) || module == self.module
    }

    /// The value of something annotated `declared`.
    #[recursive::recursive]
    fn declared(&self, declared: &Ann, env: Env<'_>, depth: u8) -> Option<Value> {
        self.step(depth)?;
        let named = |name: &str| -> Option<Value> {
            let read = |depth: u8| {
                let mut parts = name.split('.');
                let mut val = Val::Name(parts.next()?.to_string());
                for part in parts {
                    val = Val::Attr(Box::new(val), part.to_string());
                }
                self.eval(&val, Env { at: None, ..env }, depth)
            };
            // Where no scope around binds the head of the name, it is what the module
            // makes of it, the same for every annotation of the module that spells it.
            if !self.of_module(name.split('.').next().unwrap_or(name), env) {
                return read(depth + 1);
            }
            if let Some(known) = self.spelled.borrow().get(name) {
                return known.clone();
            }
            self.once_in(&self.spelled, name.to_string(), || read(0))
        };
        let type_of = |of: &Ann| match self.declared(of, env, depth + 1)? {
            Value::Instance(class) => Some(Value::Type(class)),
            Value::Either(members) => {
                let found = members.iter().map(|member| match member {
                    Value::Instance(class) => Some(Value::Type(class.clone())),
                    _ => None,
                });
                self.any(found.collect::<Option<Vec<Value>>>()?)
            }
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
                // A name that is any of several classes: an instance of any of them.
                Value::Either(members) => {
                    let found = members.iter().map(|member| match member {
                        Value::Type(class) => Some(Value::Instance(class.clone())),
                        Value::Unresolved(path) if path.form().is_none() => {
                            Some(Value::Instance(path.clone()))
                        }
                        _ => None,
                    });
                    self.any(found.collect::<Option<Vec<Value>>>()?)
                }
                _ => None,
            },
            Ann::Union(members) => {
                let mut found = Vec::new();
                for member in members {
                    if matches!(member, Ann::None) {
                        continue;
                    }
                    found.push(self.declared(member, env, depth + 1)?);
                    if found.len() > MAX_EITHER {
                        return None;
                    }
                }
                self.any(found)
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
                        Some(Ok(Form::Manager(is_async))) => {
                            let inner = self.declared(args.first()?, env, depth + 1)?;
                            Some(Value::Manager(Box::new(inner), is_async))
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
            .and_then(|facts| facts.classes.get(&*class.name))
        else {
            return Some(vec![class.clone()]);
        };
        // Each base is in the order, which holds no more classes than are followed.
        if known.bases.len() > MAX_ORDER {
            return None;
        }
        let mut direct: Vec<Class> = Vec::new();
        let mut orders: Vec<Vec<Class>> = Vec::new();
        for (index, base) in known.bases.iter().enumerate() {
            let (base, order) = match self.eval(base, Env::top(&class.module), 1) {
                Some(Value::Type(base)) => {
                    let order = self.order(&base)?;
                    (base, (*order).clone())
                }
                Some(Value::Unresolved(base)) => (base.clone(), vec![base]),
                // A base that is not followed is a class of its own, and the lookup of
                // a method ends at it. Two classes of the module share one they both
                // spell by the same dotted name, as `dict`, when the module binds the
                // head of that name no more than once: a name bound again, and a base
                // that is computed, is another class wherever it is written.
                _ => {
                    let once = |name: &String| {
                        let head = name.split('.').next().unwrap_or(name);
                        let bound = facts.as_ref().and_then(|facts| facts.bound(head));
                        bound.is_none_or(|bound| bound.len() <= 1)
                    };
                    let spelled = match dotted_val(base).filter(once) {
                        Some(name) => format!("{}:{name}", class.module),
                        None => format!("{}:{}#{index}", class.module, class.name),
                    };
                    let unknown = Class::new(NOWHERE, &spelled);
                    (unknown.clone(), vec![unknown])
                }
            };
            if direct.contains(&base) {
                return None;
            }
            direct.push(base);
            orders.push(order);
        }
        let mut merged = vec![class.clone()];
        match orders.len() {
            // One base: the class, then the order of the base, with nothing to merge.
            1 => merged.extend(orders.pop()?),
            _ => {
                orders.push(direct);
                merged.extend(c3_merge(orders)?);
            }
        }
        // The class, and as many as a merge holds when it ends.
        (merged.len() <= MAX_ORDER + 2).then_some(merged)
    }
}

/// The dotted name `val` is written as, when it is one.
fn dotted_val(val: &Val) -> Option<String> {
    match val {
        Val::Name(name) => Some(name.clone()),
        Val::Attr(base, attr) => Some(format!("{}.{attr}", dotted_val(base)?)),
        _ => None,
    }
}

/// The classes a method resolution order may hold: a longer one is not followed.
const MAX_ORDER: usize = 256;

/// The C3 merge of the orders of the bases and of the list of the bases: each time, the
/// first head that is in the tail of no order. `None` when no head qualifies.
fn c3_merge(mut orders: Vec<Vec<Class>>) -> Option<Vec<Class>> {
    let mut merged = Vec::new();
    loop {
        orders.retain(|order| !order.is_empty());
        if orders.is_empty() {
            return Some(merged);
        }
        if merged.len() > MAX_ORDER {
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
        c3_merge(orders).map(|merged| {
            merged
                .into_iter()
                .map(|class| class.name.to_string())
                .collect()
        })
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
        assert_eq!(
            found.bound("f").as_deref(),
            Some(&[Val::Function("f".to_string())][..])
        );
        assert!(found.binds("f") && !found.binds("g"));
        let declared = Val::Declared(Ann::Name("C".to_string()));
        assert_eq!(found.names["x"], [declared, Val::None]);
        assert_eq!(found.names["y"], [Val::Unknown]);
        assert_eq!(found.functions["f"].returns, Ann::Name("C".to_string()));
    }

    #[test]
    fn a_name_bound_in_a_function_or_class_is_not_a_module_name() {
        let found = facts("def f():\n    import os\n    class Local: ...\n    x = 1\nclass C:\n    class Inner: ...\n");
        let mut names: Vec<&String> = found.names.keys().chain(found.functions.keys()).collect();
        names.sort();
        assert_eq!(names, ["C", "f"]);
        let mut classes: Vec<&String> = found.classes.keys().collect();
        classes.sort();
        assert_eq!(
            classes,
            ["C"],
            "a class in a class or function is not modelled"
        );
        // Only by where it is written, for the helpers it declares itself.
        assert_eq!(found.classes["C"].attrs["Inner"], [Val::Nested(73)]);
    }

    #[test]
    fn the_names_of_a_class_are_read_without_those_of_the_others() {
        // Twice the classes: twice the names read, not four times.
        let read = |count: usize| {
            let text: String = (0..count)
                .map(|at| {
                    format!("class C{at}:\n    x = 1\n    def m(self):\n        self.y = 1\n")
                })
                .collect();
            crate::scopes::READ.with(|read| read.set(0));
            assert_eq!(facts(&text).classes.len(), count);
            crate::scopes::READ.with(Cell::get)
        };
        let (small, large) = (read(200), read(400));
        assert!(small <= 8 * 200, "{small}");
        assert!(large <= 2 * small + 16, "{small} then {large}");
    }

    #[test]
    fn a_value_of_several_classes_is_asked_once_whatever_the_number_of_calls() {
        // Eight classes, each with attributes and a method that give any of the eight:
        // a chain through them reads each class once, not once per call or per link.
        let union = "A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8";
        let steps = |calls: usize| {
            let mut text =
                "from typing import Annotated, LiteralString\nfrom sqlty import SQL\n".to_string();
            for class in 1..=8 {
                text.push_str(&format!(
                    "class A{class}:\n    nxt: \"{union}\"\n    alt: \"{union}\"\n    def step(self) -> \"{union}\": ...\n    def run(self, q: Annotated[LiteralString, SQL]): ...\n    def plain(self, q): ...\n"
                ));
            }
            for call in 0..calls {
                text.push_str(&format!(
                    "def f{call}(c, x: {union}):\n    y = x.step().nxt\n    if c:\n        y = x.alt.step()\n    y.nxt.step().alt.step().run('q')\n    y.plain('q')\n"
                ));
            }
            let source = Source::new(text.as_str(), (3, 14)).expect("parses");
            let scan = Scan::new(source.body());
            // Every class reaches a helper for `run`, the same one; none for `plain`.
            let own = crate::calls::index_functions(&source);
            assert_eq!(own.iter().count(), 8);
            let resolver = Resolver::new(&source, Some("m"), None, &own, &scan);
            STEPS.with(|steps| steps.set(0));
            let mut seen = 0;
            for &(call, scope) in &scan.calls {
                let stack = scan.stack(scope);
                let Some(attr) = call.func.as_attribute_expr() else {
                    continue;
                };
                let name = attr.attr.as_str();
                if !matches!(name, "run" | "plain") {
                    continue;
                }
                let Some(Value::Either(members)) = resolver.value_of(&attr.value, stack) else {
                    panic!("any of the classes")
                };
                assert_eq!(members.len(), 8);
                let helper = resolver.method(&attr.value, name, stack);
                assert_eq!(helper.is_some(), name == "run");
                seen += 1;
            }
            assert_eq!(seen, 2 * calls);
            STEPS.with(Cell::get)
        };
        let (one, small, large) = (steps(1), steps(200), steps(400));
        assert!(one <= 1000, "{one}");
        assert!(large - small <= small, "{small} then {large}");
        assert!(large <= 2 * small, "{small} then {large}");
        assert!((large - small) / 200 <= 80, "{small} then {large}");
    }

    #[test]
    fn a_name_bound_once_is_followed_without_walking_its_scope() {
        // A parameter and a name assigned once: no flow is kept for them, whatever the
        // number of statements around. A name assigned twice is walked.
        let text = "class Db:\n    def run(self, q): ...\ndef f(c, x: Db):\n    y = Db()\n    if c:\n        z = Db()\n    else:\n        z = x\n    x.run('q')\n    y.run('q')\n    z.run('q')\n";
        let source = Source::new(text, (3, 14)).expect("parses");
        let scan = Scan::new(source.body());
        let own = Dict::default();
        let resolver = Resolver::new(&source, Some("m"), None, &own, &scan);
        let mut flows = Vec::new();
        for &(call, scope) in &scan.calls {
            let stack = scan.stack(scope);
            let Some(attr) = call.func.as_attribute_expr() else {
                continue;
            };
            let found = resolver.value_of(&attr.value, stack);
            assert_eq!(found, Some(Value::Instance(Class::new("m", "Db"))));
            let kept = resolver.kept.borrow();
            flows.push(kept.get(&scope).map_or(0, |kept| kept.flows.len()));
        }
        assert_eq!(flows, [0, 0, 1]);
    }

    #[test]
    fn what_is_kept_of_a_scope_is_dropped_once_its_calls_are_read() {
        // Every function binds a name twice, so a flow is kept for it: once the calls of
        // the function are read, nothing of it is left, whatever the number of functions.
        // What is kept of the module and of the function around a nested one stays.
        let class = "class Db:\n    def run(self, q): ...\n";
        let one = |at: usize| {
            format!(
                "def f{at}(c):\n    x = Db()\n    if c:\n        x = Db()\n    x.run('q')\n    \
                 def g():\n        y = Db()\n        if c:\n            y = Db()\n        \
                 y.run('q')\n    x.run('q')\n"
            )
        };
        let functions: String = (0..50).map(one).collect();
        let text =
            format!("{class}m = Db()\nif m:\n    m = Db()\nm.run('q')\n{functions}m.run('q')\n");
        let source = Source::new(text.as_str(), (3, 14)).expect("parses");
        let scan = Scan::new(source.body());
        let own = Dict::default();
        let resolver = Resolver::new(&source, Some("m"), None, &own, &scan);
        let (mut most, mut read) = (0, 0);
        for &(call, scope) in &scan.calls {
            let stack = scan.stack(scope);
            let Some(attr) = call.func.as_attribute_expr() else {
                continue;
            };
            let found = resolver.value_of(&attr.value, stack);
            assert_eq!(found, Some(Value::Instance(Class::new("m", "Db"))));
            let kept = resolver.kept.borrow();
            // The scopes around the call, and the module.
            assert!(kept.keys().all(|kept| *kept == 0 || stack.contains(kept)));
            assert!(stack.iter().all(|around| kept[around].flows.len() == 1));
            assert!((1..=2).contains(&kept[&0].flows.len()));
            most = most.max(kept.len());
            read += 1;
        }
        assert_eq!((most, read), (3, 2 + 3 * 50));
        // A scope read again after it was dropped gives the same.
        let mut inner = scan.calls.iter().filter(|(_, scope)| *scope != 0);
        let again = inner.find_map(|(call, scope)| Some((call.func.as_attribute_expr()?, *scope)));
        let (attr, scope) = again.expect("x.run");
        let found = resolver.value_of(&attr.value, scan.stack(scope));
        assert_eq!(found, Some(Value::Instance(Class::new("m", "Db"))));
        assert_eq!(resolver.kept.borrow().len(), 2);
    }

    #[test]
    fn a_function_defined_once_is_bound_as_any_other_name() {
        // It is kept as a function only: `bound` and `binds` answer as for a name
        // assigned, so that a reader need not know where each is kept.
        let found = facts("def f(): ...\ndef g(): ...\ng = f\nx = f\n");
        assert!(found.binds("f") && found.binds("g") && found.binds("x"));
        assert!(!found.binds("y") && found.bound("y").is_none());
        assert_eq!(found.bound("f").map(|bound| bound.len()), Some(1));
        assert_eq!(found.bound("g").map(|bound| bound.len()), Some(2));
        let name = [Val::Name("f".to_string())];
        assert_eq!(found.bound("x").as_deref(), Some(&name[..]));
    }

    #[test]
    fn a_self_attribute_is_what_its_method_binds_it_to() {
        let found = facts(
            "class C:\n    kind: K\n    def __init__(self, db: Db, other):\n        self.db = db\n        self.other = other\n        self.made = make()\n        local = make()\n        self.local = local\n        self.me = self\n    def one(self): ...\n    one = None\n",
        );
        let attrs = &found.classes["C"].attrs;
        assert_eq!(attrs["kind"], [Val::Declared(Ann::Name("K".to_string()))]);
        assert_eq!(attrs["db"], [Val::Typed(Ann::Name("Db".to_string()))]);
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

    #[test]
    fn cast_is_named_through_an_await() {
        let parsed = crate::parse::expression("(await cast)(DB, x)").expect("parses");
        let cast = Val::Awaited(Box::new(Val::Name("cast".to_string())));
        let to = Some(Ann::Name("DB".to_string()));
        assert_eq!(
            value(&parsed.syntax().body, 0),
            Val::Call(Box::new(cast), to)
        );
    }

    #[test]
    fn an_awaited_value_in_a_class_body_is_read_as_any_other() {
        // Python refuses `await` there; the parser does not.
        let found = facts("class C:\n    D = int\n    x = await f()\n    y = await D()\n");
        let called = Val::Call(Box::new(Val::Name("f".to_string())), None);
        let attrs = &found.classes["C"].attrs;
        assert_eq!(attrs["x"], [Val::Awaited(Box::new(called))]);
        assert_eq!(attrs["y"], [Val::Unknown], "`D` is what the body binds");
    }

    #[test]
    fn a_class_of_more_bases_than_are_followed_has_no_order() {
        // The helper is a method of the first base: found through the order only.
        let found = |bases: usize| {
            let mut text = "from typing import Annotated, LiteralString\nfrom sqlty import SQL\nclass B0:\n    def run(self, q: Annotated[LiteralString, SQL]): ...\n".to_string();
            for base in 1..bases {
                text.push_str(&format!("class B{base}: ...\n"));
            }
            let names: Vec<String> = (0..bases).map(|base| format!("B{base}")).collect();
            text.push_str(&format!(
                "class C({}): ...\ndef f(c: C):\n    c.run('q')\n",
                names.join(", ")
            ));
            let source = Source::new(text.as_str(), (3, 14)).expect("parses");
            let scan = Scan::new(source.body());
            let own = crate::calls::index_functions(&source);
            let resolver = Resolver::new(&source, Some("m"), None, &own, &scan);
            let &(call, scope) = scan.calls.last().expect("a call");
            let attr = call.func.as_attribute_expr().expect("a method");
            resolver
                .method(&attr.value, "run", scan.stack(scope))
                .is_some()
        };
        assert!(found(MAX_ORDER));
        assert!(!found(MAX_ORDER + 1));
    }

    #[test]
    fn an_order_of_more_classes_than_are_followed_is_not_merged() {
        let orders = |count: usize| -> Vec<Vec<Class>> {
            (0..count)
                .map(|at| vec![class(&format!("C{at}"))])
                .collect()
        };
        let merged = c3_merge(orders(MAX_ORDER + 1)).expect("merges");
        assert_eq!(merged.len(), MAX_ORDER + 1);
        assert_eq!(c3_merge(orders(MAX_ORDER + 2)), None);
    }
}
