//! The scopes of a module and the classes its names and `self` attributes hold, which
//! [`crate::arg_types`] reads argument types from.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use crate::frozen::Table;
use ruff_python_ast::visitor::{walk_except_handler, walk_expr, walk_pattern, walk_stmt, Visitor};
use ruff_python_ast::{
    self as ast, Expr, ExprCall, ExprTString, Number, Operator, Stmt, StmtClassDef,
    StmtFunctionDef, UnaryOp,
};
use ruff_text_size::Ranged;

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
    /// The text of a `str` literal, or of a name bound once to one.
    pub text: Option<String>,
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

#[cfg(test)]
thread_local! {
    /// The walks [`Scan::new`] has made on this thread.
    pub(crate) static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// What one walk of a module collects: its scopes, the classes names hold, every call and
/// t-string with its innermost scope, and every string literal.
pub struct Scan<'a> {
    /// The scopes, by id.
    pub scopes: Vec<Node<'a>>,
    /// The scopes around each scope's contents, innermost last, classes included.
    paths: Vec<Vec<Scope>>,
    stack: Vec<Scope>,
    pub(crate) types: HashMap<(Scope, &'a str), Option<String>>,
    /// The parameters of each function its body does not assign.
    pub(crate) parameters: HashSet<(Scope, &'a str)>,
    /// The type parameters of each generic function and class.
    pub(crate) generics: HashSet<(Scope, &'a str)>,
    pub(crate) attributes: HashMap<(Scope, &'a str), Option<String>>,
    /// The first `str` literal each scope assigns to a name alone: `name = "..."`.
    pub(crate) texts: HashMap<(Scope, &'a str), &'a ast::ExprStringLiteral>,
    operands: HashSet<usize>,
    /// Every call, in visit order, with its innermost scope.
    pub calls: Vec<(&'a ExprCall, Scope)>,
    /// Every t-string, with its innermost scope.
    pub templates: Vec<(&'a ExprTString, Scope)>,
    /// Every string literal, f-string and t-string, and whether it is an operand of a `+`.
    pub strings: Vec<(&'a Expr, bool)>,
}

impl<'a> Scan<'a> {
    /// Walks `body`.
    pub fn new(body: &'a [Stmt]) -> Self {
        #[cfg(test)]
        WALKS.with(|count| count.set(count.get() + 1));
        let mut walker = Walker {
            scan: Self {
                scopes: vec![Node::Module],
                paths: vec![Vec::new()],
                stack: Vec::new(),
                types: HashMap::new(),
                parameters: HashSet::new(),
                generics: HashSet::new(),
                attributes: HashMap::new(),
                texts: HashMap::new(),
                operands: HashSet::new(),
                calls: Vec::new(),
                templates: Vec::new(),
                strings: Vec::new(),
            },
        };
        walker.visit_body(body);
        walker.scan
    }

    /// The scopes inside `scope`, outermost first, `scope` last; empty for the module.
    pub fn stack(&self, scope: Scope) -> &[Scope] {
        &self.paths[scope]
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
    /// Records that the innermost function or class binds `names` to what no annotation
    /// says: an import, `for`, `with`, `def`, `class`, `except`, `match` or `:=`. The module
    /// counts its own bindings ([`Source::bound`](crate::types::symbols::Source::bound)).
    fn bind_unknown(&mut self, names: impl IntoIterator<Item = &'a str>) {
        let Some(&scope) = self.stack.last() else {
            return;
        };
        for name in names {
            self.parameters.remove(&(scope, name));
            self.types.insert((scope, name), None);
        }
    }

    fn generic(&mut self, scope: Scope, params: Option<&'a ast::TypeParams>) {
        for param in params.iter().flat_map(|params| &params.type_params) {
            self.generics.insert((scope, param.name().as_str()));
        }
    }

    /// Records `name = Cls(...)`, `name: Cls` and `self.attr = ...` in a method, as `_Scan._typed`.
    fn typed(&mut self, targets: &[&'a Expr], kind: &Option<String>) {
        let scope = self.stack.last().copied().unwrap_or(0);
        for target in targets {
            match target {
                Expr::Name(name) => {
                    let key = (scope, name.id.as_str());
                    self.parameters.remove(&key);
                    note(&mut self.types, key, kind.clone());
                }
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

struct Walker<'a> {
    scan: Scan<'a>,
}

impl<'a> Visitor<'a> for Walker<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                self.scan.bind_unknown([node.name.as_str()]);
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
                    let key = (scope, parameter.parameter.name.as_str());
                    self.scan.parameters.insert(key);
                    note(&mut self.scan.types, key, kind);
                }
                self.scan.generic(scope, node.type_params.as_deref());
                self.scan.stack.push(scope);
                self.scan.paths.push(self.scan.stack.clone());
                walk_stmt(self, stmt);
                self.scan.stack.pop();
                return;
            }
            Stmt::ClassDef(node) => {
                self.scan.bind_unknown([node.name.as_str()]);
                let scope = self.scan.scopes.len();
                self.scan.scopes.push(Node::Class(node));
                self.scan.generic(scope, node.type_params.as_deref());
                self.scan.stack.push(scope);
                self.scan.paths.push(self.scan.stack.clone());
                walk_stmt(self, stmt);
                self.scan.stack.pop();
                return;
            }
            Stmt::Assign(node) => {
                let kind = match node.value.as_ref() {
                    Expr::Call(call) => class_name(Some(&call.func)),
                    value => literal_type(value).map(ToString::to_string),
                };
                if let ([Expr::Name(name)], Expr::StringLiteral(text)) =
                    (node.targets.as_slice(), node.value.as_ref())
                {
                    let scope = self.scan.stack.last().copied().unwrap_or(0);
                    self.scan
                        .texts
                        .entry((scope, name.id.as_str()))
                        .or_insert(text);
                }
                let targets: Vec<&Expr> = node.targets.iter().collect();
                self.scan.typed(&targets, &kind);
            }
            Stmt::Import(node) => self.scan.bind_unknown(node.names.iter().map(|alias| {
                let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                name.split('.').next().unwrap_or(name)
            })),
            Stmt::ImportFrom(node) => self.scan.bind_unknown(
                node.names
                    .iter()
                    .map(|alias| alias.asname.as_ref().unwrap_or(&alias.name).as_str()),
            ),
            Stmt::For(node) => self.scan.bind_unknown(stored(&node.target)),
            Stmt::With(node) => self.scan.bind_unknown(
                node.items
                    .iter()
                    .filter_map(|item| item.optional_vars.as_deref())
                    .flat_map(stored),
            ),
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
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_except_handler(&mut self, handler: &'a ast::ExceptHandler) {
        let ast::ExceptHandler::ExceptHandler(node) = handler;
        self.scan
            .bind_unknown(node.name.as_ref().map(ruff_python_ast::Identifier::as_str));
        walk_except_handler(self, handler);
    }

    fn visit_pattern(&mut self, pattern: &'a ast::Pattern) {
        if let ast::Pattern::MatchAs(ast::PatternMatchAs {
            name: Some(name), ..
        })
        | ast::Pattern::MatchStar(ast::PatternMatchStar {
            name: Some(name), ..
        })
        | ast::Pattern::MatchMapping(ast::PatternMatchMapping {
            rest: Some(name), ..
        }) = pattern
        {
            self.scan.bind_unknown([name.as_str()]);
        }
        walk_pattern(self, pattern);
    }

    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        let scope = self.scan.stack.last().copied().unwrap_or(0);
        match expr {
            Expr::Named(named) => self.scan.bind_unknown(stored(&named.target)),
            Expr::Call(call) => self.scan.calls.push((call, scope)),
            Expr::TString(template) => {
                self.scan.templates.push((template, scope));
                self.string(expr);
            }
            Expr::BinOp(op) if op.op == Operator::Add => {
                self.scan.operands.insert(op.left.start().to_usize());
                self.scan.operands.insert(op.right.start().to_usize());
            }
            Expr::StringLiteral(_) | Expr::FString(_) => self.string(expr),
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// The names a target binds: `a`, `(a, *b)` or `[a, b]`.
fn stored(target: &Expr) -> Vec<&str> {
    match target {
        Expr::Name(name) => vec![name.id.as_str()],
        Expr::Tuple(ast::ExprTuple { elts, .. }) | Expr::List(ast::ExprList { elts, .. }) => {
            elts.iter().flat_map(stored).collect()
        }
        Expr::Starred(starred) => stored(&starred.value),
        _ => Vec::new(),
    }
}

impl<'a> Walker<'a> {
    fn string(&mut self, expr: &'a Expr) {
        let operand = self.scan.operands.contains(&expr.start().to_usize());
        self.scan.strings.push((expr, operand));
    }
}

/// The builtin class of a literal other than `None`.
pub(crate) fn literal_type(expr: &Expr) -> Option<&'static str> {
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
    fn a_function_binds_imports_and_loop_targets_to_no_class() {
        let text = "import os\ndef f(a: int):\n    import os.path, json as a\n    \
                    for (b, *c), d.e in []:\n        pass\n    g: int = 1\n";
        let parsed = parse_module(text).unwrap();
        let scan = Scan::new(&parsed.syntax().body);
        let mut bound: Vec<&str> = scan
            .types
            .iter()
            .filter(|(&(scope, _), kind)| scope == 1 && kind.is_none())
            .map(|(&(_, name), _)| name)
            .collect();
        bound.sort_unstable();
        assert_eq!(bound, ["a", "b", "c", "os"]);
        assert!(!scan.parameters.contains(&(1, "a")));
        assert_eq!(scan.types[&(1, "g")].as_deref(), Some("int"));
        assert!(!scan.types.contains_key(&(0, "os")));
    }

    #[test]
    fn lines_end_at_newline_carriage_return_or_both() {
        let lines = Lines::new("a\nb\r\nc\rd");
        let found: Vec<usize> = [0, 2, 5, 7].map(|offset| lines.line(offset)).into();
        assert_eq!(found, [1, 2, 3, 4]);
    }
}
