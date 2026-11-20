//! The scopes of a module, the classes its names and `self` attributes hold, and the
//! argument types read from them.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use crate::frozen::Table;
use crate::types::symbols::Source;
use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{
    self as ast, Expr, ExprCall, ExprTString, Number, Stmt, StmtClassDef, StmtFunctionDef, UnaryOp,
};
use ruff_text_size::{Ranged, TextSize};

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

/// What one walk of a module collects: its scopes, the classes names hold, and the calls
/// `want` keeps, each with the scopes around it (innermost last, classes included).
pub struct Scan<'a> {
    /// The scopes, by id.
    pub scopes: Vec<Node<'a>>,
    stack: Vec<Scope>,
    types: HashMap<(Scope, &'a str), Option<String>>,
    attributes: HashMap<(Scope, &'a str), Option<String>>,
    /// What each name of a function is bound to, for [`crate::receivers`].
    bound: HashMap<(Scope, &'a str), Vec<Binding<'a>>>,
    /// The end of the block being walked, and the outermost loop of the function around it.
    until: u32,
    looped: Option<(u32, u32)>,
    /// The calls kept, with their scopes.
    pub calls: Vec<(&'a ExprCall, Vec<Scope>)>,
    /// Every t-string, with its scopes.
    pub templates: Vec<(&'a ExprTString, Vec<Scope>)>,
}

impl<'a> Scan<'a> {
    /// Walks `body`, keeping the calls `want` accepts.
    pub fn new(body: &'a [Stmt], want: impl FnMut(&'a ExprCall) -> bool) -> Self {
        let mut walker = Walker {
            scan: Self {
                scopes: vec![Node::Module],
                stack: Vec::new(),
                types: HashMap::new(),
                attributes: HashMap::new(),
                bound: HashMap::new(),
                until: 0,
                looped: None,
                calls: Vec::new(),
                templates: Vec::new(),
            },
            want,
        };
        walker.visit_body(body);
        walker.scan
    }

    /// Whether `scope` is a class.
    pub fn is_class(&self, scope: Scope) -> bool {
        matches!(self.scopes[scope], Node::Class(_))
    }

    /// What `name` can hold in the function `scope` at offset `at` (anywhere without one):
    /// the last binding every path to `at` runs through, the bindings since, and those of
    /// a loop around `at`. `None` when the scope does not bind it.
    pub fn bound(&self, scope: Scope, name: &str, at: Option<u32>) -> Option<Vec<Origin<'a>>> {
        let all = self.bound.get(&(scope, name))?;
        let origins = |kept: &mut dyn Iterator<Item = &Binding<'a>>| {
            Some(kept.map(|binding| binding.origin).collect())
        };
        // A declaration holds for the whole scope.
        let mut declared = all
            .iter()
            .filter(|binding| matches!(binding.origin, Origin::Declared(_)));
        if declared.clone().next().is_some() {
            return origins(&mut declared);
        }
        let Some(at) = at else {
            return origins(&mut all.iter());
        };
        let last = all
            .iter()
            .filter(|binding| binding.at <= at && at < binding.until)
            .map(|binding| binding.at)
            .max();
        origins(&mut all.iter().filter(|binding| {
            if binding.at <= at {
                last.is_none_or(|last| binding.at >= last)
            } else {
                binding
                    .looped
                    .is_some_and(|(start, end)| start <= at && at < end)
            }
        }))
    }

    /// Records a binding of `target`, in effect from `at`, in the innermost scope when that
    /// is a function.
    fn bind(&mut self, target: &'a Expr, origin: Origin<'a>, at: TextSize) {
        let Some(&scope) = self.stack.last() else {
            return;
        };
        if self.is_class(scope) {
            return;
        }
        match target {
            Expr::Name(name) => {
                let binding = Binding {
                    origin,
                    at: at.to_u32(),
                    until: self.until,
                    looped: self.looped,
                };
                self.bound
                    .entry((scope, name.id.as_str()))
                    .or_default()
                    .push(binding);
            }
            Expr::Tuple(ast::ExprTuple { elts, .. }) | Expr::List(ast::ExprList { elts, .. }) => {
                for item in elts {
                    self.bind(item, Origin::Unknown, at);
                }
            }
            Expr::Starred(starred) => self.bind(&starred.value, Origin::Unknown, at),
            _ => {}
        }
    }
}

/// Where a name of a function gets its value.
#[derive(Clone, Copy)]
pub enum Origin<'a> {
    /// `name: T`, a parameter or a variable: the annotation.
    Declared(&'a Expr),
    /// `name = value`.
    Value(&'a Expr),
    /// `with value as name`, `async with` when true.
    Entered(&'a Expr, bool),
    /// A parameter without annotation, a loop variable, an unpacked value.
    Unknown,
}

/// A binding of a name: in effect from `at`, on every path through its block, which ends
/// at `until`; `looped` is the outermost loop of its function around it.
struct Binding<'a> {
    origin: Origin<'a>,
    at: u32,
    until: u32,
    looped: Option<(u32, u32)>,
}

/// Remembers the class a name holds; `None` once it is ambiguous, as `_Scan._note` does.
fn note<'a>(
    types: &mut HashMap<(Scope, &'a str), Option<String>>,
    key: (Scope, &'a str),
    kind: Option<String>,
) {
    match types.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(kind);
        }
        Entry::Occupied(mut entry) => {
            if *entry.get() != kind {
                entry.insert(None);
            }
        }
    }
}

impl<'a> Scan<'a> {
    /// Records `name = Cls(...)`, `name: Cls` and `self.attr = ...` in a method, as `_Scan._typed`.
    fn typed(&mut self, targets: &[&'a Expr], kind: &Option<String>) {
        let scope = self.stack.last().copied().unwrap_or(0);
        for target in targets {
            match target {
                Expr::Name(name) => note(&mut self.types, (scope, name.id.as_str()), kind.clone()),
                Expr::Attribute(attr) => {
                    let Expr::Name(base) = attr.value.as_ref() else {
                        continue;
                    };
                    let [.., class, method] = self.stack[..] else {
                        continue;
                    };
                    if let (Node::Class(_), Node::Function(function)) =
                        (self.scopes[class], self.scopes[method])
                    {
                        if function
                            .parameters
                            .args
                            .first()
                            .is_some_and(|first| first.parameter.name.as_str() == base.id.as_str())
                        {
                            note(
                                &mut self.attributes,
                                (class, attr.attr.as_str()),
                                kind.clone(),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

struct Walker<'a, W> {
    scan: Scan<'a>,
    want: W,
}

impl<'a, W: FnMut(&'a ExprCall) -> bool> Visitor<'a> for Walker<'a, W> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                let scope = self.scan.scopes.len();
                self.scan.scopes.push(Node::Function(node));
                let parameters = &node.parameters;
                for parameter in parameters
                    .posonlyargs
                    .iter()
                    .chain(&parameters.args)
                    .chain(&parameters.kwonlyargs)
                {
                    let kind = class_name(parameter.parameter.annotation.as_deref());
                    note(
                        &mut self.scan.types,
                        (scope, parameter.parameter.name.as_str()),
                        kind,
                    );
                }
                for parameter in parameters {
                    let origin = parameter
                        .annotation()
                        .filter(|_| !parameter.is_variadic())
                        .map_or(Origin::Unknown, Origin::Declared);
                    let binding = Binding {
                        origin,
                        at: node.start().to_u32(),
                        until: node.end().to_u32(),
                        looped: None,
                    };
                    self.scan
                        .bound
                        .entry((scope, parameter.name().as_str()))
                        .or_default()
                        .push(binding);
                }
                self.scan.stack.push(scope);
                let looped = self.scan.looped.take();
                walk_stmt(self, stmt);
                self.scan.looped = looped;
                self.scan.stack.pop();
                return;
            }
            Stmt::ClassDef(node) => {
                let scope = self.scan.scopes.len();
                self.scan.scopes.push(Node::Class(node));
                self.scan.stack.push(scope);
                walk_stmt(self, stmt);
                self.scan.stack.pop();
                return;
            }
            Stmt::Assign(node) => {
                let kind = match node.value.as_ref() {
                    Expr::Call(call) => class_name(Some(&call.func)),
                    value => literal_type(value).map(ToString::to_string),
                };
                let targets: Vec<&Expr> = node.targets.iter().collect();
                self.scan.typed(&targets, &kind);
                for target in targets {
                    self.scan
                        .bind(target, Origin::Value(&node.value), node.end());
                }
            }
            Stmt::AnnAssign(node) => {
                let annotation = match node.annotation.as_ref() {
                    Expr::Subscript(subscript) => subscript.value.as_ref(),
                    annotation => annotation,
                };
                let final_ = matches!(
                    class_name(Some(annotation)).as_deref(),
                    Some("Final" | "typing.Final" | "typing_extensions.Final")
                );
                let kind = if final_ {
                    node.value
                        .as_deref()
                        .and_then(literal_type)
                        .map(ToString::to_string)
                } else {
                    class_name(Some(&node.annotation))
                }
                .or_else(|| match node.value.as_deref() {
                    Some(Expr::Call(call)) => class_name(Some(&call.func)),
                    _ => None,
                });
                self.scan.typed(&[&node.target], &kind);
                let origin = match &node.value {
                    Some(value) if final_ && std::ptr::eq(annotation, node.annotation.as_ref()) => {
                        Origin::Value(value)
                    }
                    _ => Origin::Declared(&node.annotation),
                };
                self.scan.bind(&node.target, origin, node.end());
            }
            Stmt::With(node) => {
                for item in &node.items {
                    if let Some(target) = &item.optional_vars {
                        self.scan.bind(
                            target,
                            Origin::Entered(&item.context_expr, node.is_async),
                            item.context_expr.end(),
                        );
                    }
                }
            }
            Stmt::AugAssign(node) => self.scan.bind(&node.target, Origin::Unknown, node.end()),
            _ => {}
        }
        let looped = matches!(stmt, Stmt::For(_) | Stmt::While(_)) && self.scan.looped.is_none();
        if looped {
            self.scan.looped = Some((stmt.start().to_u32(), stmt.end().to_u32()));
        }
        if let Stmt::For(node) = stmt {
            self.scan
                .bind(&node.target, Origin::Unknown, node.target.end());
        }
        walk_stmt(self, stmt);
        if looped {
            self.scan.looped = None;
        }
    }

    fn visit_body(&mut self, body: &'a [Stmt]) {
        let until = self.scan.until;
        if let Some(last) = body.last() {
            self.scan.until = last.end().to_u32();
        }
        for stmt in body {
            self.visit_stmt(stmt);
        }
        self.scan.until = until;
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
            _ => {}
        }
        walk_expr(self, expr);
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

    /// Whether `name` is a parameter of, or assigned in, the module or a function of `stack`.
    fn binds(&self, stack: &[Scope], name: &str) -> bool {
        stack
            .iter()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .any(|&scope| self.scan.types.contains_key(&(scope, name)))
    }

    /// The class a variable holds in `stack`.
    pub fn type_of(&self, stack: &[Scope], name: &str) -> Option<String> {
        stack
            .iter()
            .rev()
            .filter(|&&scope| !self.scan.is_class(scope))
            .chain(std::iter::once(&0))
            .find_map(|&scope| self.scan.types.get(&(scope, name)))
            .cloned()
            .flatten()
    }

    /// The class `self.name` holds in the innermost class of `stack`.
    pub fn attribute_type(&self, stack: &[Scope], name: &str) -> Option<String> {
        let class = stack
            .iter()
            .rev()
            .find(|&&scope| self.scan.is_class(scope))?;
        self.scan.attributes.get(&(*class, name)).cloned().flatten()
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
