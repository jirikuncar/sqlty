//! The scopes of a module and the classes its names and `self` attributes hold, as
//! `sqlty._source._Scan` collects them, and the argument types `sqlty._params` reads from them.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use ruff_python_ast::visitor::{walk_expr, walk_stmt, Visitor};
use ruff_python_ast::{
    Expr, ExprCall, ExprTString, Number, Stmt, StmtClassDef, StmtFunctionDef, UnaryOp,
};

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
                self.scan.stack.push(scope);
                walk_stmt(self, stmt);
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
                    _ => None,
                };
                let targets: Vec<&Expr> = node.targets.iter().collect();
                self.scan.typed(&targets, &kind);
            }
            Stmt::AnnAssign(node) => {
                let kind =
                    class_name(Some(&node.annotation)).or_else(|| match node.value.as_deref() {
                        Some(Expr::Call(call)) => class_name(Some(&call.func)),
                        _ => None,
                    });
                self.scan.typed(&[&node.target], &kind);
            }
            _ => {}
        }
        walk_stmt(self, stmt);
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

/// Reads argument types in the scopes of a [`Scan`].
pub struct Context<'a, 's> {
    /// `ParsedSource.imported_names`.
    pub imported: &'s HashMap<String, String>,
    /// The scan of the module.
    pub scan: &'s Scan<'a>,
}

impl Context<'_, '_> {
    /// The arguments a tuple or list literal, or a dict literal of string keys, passes, as
    /// `sqlty._params.literal_params` reads them.
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

    /// The type `expr` has where the source says so, as `sqlty._params.arg_type` reads it.
    pub fn arg_type(&self, expr: &Expr, stack: &[Scope]) -> Option<String> {
        let name = match expr {
            Expr::NoneLiteral(_) => "None",
            Expr::BooleanLiteral(_) => "bool",
            Expr::NumberLiteral(number) => match number.value {
                Number::Int(_) => "int",
                Number::Float(_) => "float",
                Number::Complex { .. } => return None,
            },
            Expr::StringLiteral(_) | Expr::FString(_) => "str",
            Expr::BytesLiteral(_) => "bytes",
            Expr::UnaryOp(unary) if matches!(unary.op, UnaryOp::UAdd | UnaryOp::USub) => {
                match unary.operand.as_ref() {
                    Expr::NumberLiteral(number) => match number.value {
                        Number::Int(_) => "int",
                        Number::Float(_) => "float",
                        Number::Complex { .. } => return None,
                    },
                    Expr::BooleanLiteral(_) => "int",
                    _ => return None,
                }
            }
            Expr::List(_) | Expr::Tuple(_) | Expr::Set(_) => {
                let items = match expr {
                    Expr::List(list) => &list.elts,
                    Expr::Tuple(tuple) => &tuple.elts,
                    Expr::Set(set) => &set.elts,
                    _ => unreachable!(),
                };
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
            Expr::Name(name) => return self.class(self.type_of(stack, name.id.as_str())),
            Expr::Call(call) => return self.class(dotted(&call.func)),
            _ => return None,
        };
        Some(name.to_string())
    }

    /// The class a variable holds in `stack`, as `ParsedSource.type_of` reads it.
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

/// `sqlty._params._class`: the type of an instance of the class spelled `name`, when it is
/// a known one.
pub fn class_of(name: Option<String>, imported: &HashMap<String, String>) -> Option<String> {
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

    /// The number of line starts.
    pub fn count(&self) -> usize {
        self.0.len()
    }

    /// The line `offset` is on.
    pub fn line(&self, offset: usize) -> usize {
        self.0.partition_point(|&start| start <= offset)
    }

    /// The offset of the start of the 1-based `line`, or `len` past the last one.
    pub fn start(&self, line: usize, len: usize) -> usize {
        self.0.get(line - 1).copied().unwrap_or(len)
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
pub fn qualified(name: Option<String>, imported: &HashMap<String, String>) -> Option<String> {
    let name = name?;
    let (head, rest) = name.split_once('.').unwrap_or((&name, ""));
    let found = imported.get(head)?;
    Some(if rest.is_empty() {
        found.clone()
    } else {
        format!("{found}.{rest}")
    })
}

/// The dotted name an annotation or a called expression spells, as `_class_name` reads it.
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
