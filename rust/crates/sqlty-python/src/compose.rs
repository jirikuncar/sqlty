//! `psycopg.sql` compositions built only from literals, rendered as `sqlty._psycopg` does
//! with the real classes: the expression is evaluated on a model of `SQL`, `Identifier`,
//! `Literal`, `Placeholder` and `Composed`, of `str.format` and `str.join`. No user code
//! runs.
//!
//! Where the model does not cover what Python would compute (a float in a format field, a
//! list as a literal), evaluation stops with [`Stop::Unread`] and the caller leaves the
//! module to Python.

use std::collections::{HashMap, HashSet};

use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, ExprContext, ExprName, Number, Operator, Parameters, Stmt};
use ruff_text_size::Ranged;

use crate::scopes::dotted;
use crate::types::info::Dict;
use crate::types::symbols::Source;

const CLASSES: [&str; 5] = ["SQL", "Identifier", "Literal", "Placeholder", "Composed"];
const MUTATORS: [&str; 8] = [
    "append", "extend", "insert", "pop", "remove", "clear", "sort", "reverse",
];
/// `sqlty._psycopg.UNCHECKED`: what `render` gives for a composition with a dynamic part.
pub const UNCHECKED: &str = "unchecked";

/// The query of a `SQL[R]("...")` call, with the `P` of `SQL[R, P]` and its source.
#[derive(Clone)]
pub struct Declared<'a> {
    pub query: String,
    pub params: Option<&'a Expr>,
    pub source: &'a Source,
}

impl std::fmt::Debug for Declared<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Declared({:?})", self.query)
    }
}

/// A value an expression takes.
#[derive(Clone, Debug)]
pub enum Value<'a> {
    None,
    Bool(bool),
    /// The decimal digits, when the literal is short enough to have them at hand.
    Int(Option<String>),
    Float(f64),
    Str(String),
    Declared(Declared<'a>),
    Bytes,
    /// `...` or a complex number.
    Other,
    List(Vec<Value<'a>>),
    Dict(Vec<(String, Value<'a>)>),
    Sql(String),
    Identifier(Vec<String>),
    Placeholder(String, char),
    /// Its parts: composables, or any other value as a `Literal`.
    Composed(Vec<Value<'a>>),
}

/// Why an expression has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// `_Dynamic`: a part is not a literal.
    Dynamic,
    /// An exception Python raises and the passes catch: `TypeError`, `ValueError`.
    Error,
    /// Python computes something this model does not.
    Unread,
}

type Eval<T> = Result<T, Stop>;

impl<'a> Value<'a> {
    pub fn is_composable(&self) -> bool {
        matches!(
            self,
            Value::Sql(_) | Value::Identifier(_) | Value::Placeholder(..) | Value::Composed(_)
        )
    }

    /// The text of a `str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            Value::Declared(declared) => Some(&declared.query),
            _ => None,
        }
    }

    /// What tells two values apart, as `repr` does in Python.
    pub fn key(&self, out: &mut String) {
        match self {
            Value::None => out.push('N'),
            Value::Bool(value) => out.push(if *value { 'T' } else { 'F' }),
            Value::Int(digits) => {
                out.push('i');
                out.push_str(digits.as_deref().unwrap_or("?"));
            }
            Value::Float(value) => out.push_str(&format!("f{:x}", value.to_bits())),
            Value::Str(_) | Value::Declared(_) => {
                let text = self.as_str().unwrap_or_default();
                out.push_str(&format!("s{}:{text}", text.len()));
            }
            Value::Bytes => out.push('b'),
            Value::Other => out.push('o'),
            Value::List(items) => {
                out.push('[');
                items.iter().for_each(|item| item.key(out));
                out.push(']');
            }
            Value::Dict(items) => {
                out.push('{');
                for (key, value) in items {
                    out.push_str(&format!("{}:{key}", key.len()));
                    value.key(out);
                }
                out.push('}');
            }
            Value::Sql(text) => out.push_str(&format!("S{}:{text}", text.len())),
            Value::Identifier(parts) => {
                out.push('I');
                for part in parts {
                    out.push_str(&format!("{}:{part}", part.len()));
                }
                out.push(';');
            }
            Value::Placeholder(name, code) => {
                out.push_str(&format!("P{code}{}:{name}", name.len()));
            }
            Value::Composed(parts) => {
                out.push('(');
                parts.iter().for_each(|part| part.key(out));
                out.push(')');
            }
        }
    }

    /// `Composable.as_string(None)`.
    pub fn as_string(&self) -> Eval<String> {
        let mut out = String::new();
        self.write(&mut out)?;
        Ok(out)
    }

    fn write(&self, out: &mut String) -> Eval<()> {
        match self {
            Value::Sql(text) => out.push_str(text),
            Value::Identifier(parts) => {
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push('.');
                    }
                    out.push('"');
                    out.push_str(&part.replace('"', "\"\""));
                    out.push('"');
                }
            }
            Value::Placeholder(name, code) => {
                if name.is_empty() {
                    out.push('%');
                } else {
                    out.push_str(&format!("%({name})"));
                }
                out.push(*code);
            }
            Value::Composed(parts) => {
                for part in parts {
                    if part.is_composable() {
                        part.write(out)?;
                    } else {
                        part.literal(out)?;
                    }
                }
            }
            _ => return Err(Stop::Error),
        }
        Ok(())
    }

    /// `Literal(self).as_string(None)`, for the values whose dumper is modelled.
    fn literal(&self, out: &mut String) -> Eval<()> {
        match self {
            Value::None => out.push_str("NULL"),
            Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Value::Int(Some(digits)) => {
                if digits.starts_with('-') {
                    out.push(' ');
                }
                out.push_str(digits);
            }
            Value::Str(_) | Value::Declared(_) => {
                let text = self.as_str().unwrap_or_default();
                if text.contains('\0') {
                    return Err(Stop::Unread);
                }
                let quoted = text.replace('\'', "''");
                if text.contains('\\') {
                    out.push_str(&format!(" E'{}'", quoted.replace('\\', "\\\\")));
                } else {
                    out.push_str(&format!("'{quoted}'"));
                }
            }
            _ => return Err(Stop::Unread),
        }
        Ok(())
    }

    /// `iter(self)` for what `Composed(...)` and `join(...)` take.
    fn items(self) -> Eval<Vec<Value<'a>>> {
        match self {
            Value::List(items) | Value::Composed(items) => Ok(items),
            Value::Str(_) | Value::Declared(_) => Ok(self
                .as_str()
                .unwrap_or_default()
                .chars()
                .map(|c| Value::Str(c.to_string()))
                .collect()),
            Value::Dict(items) => Ok(items.into_iter().map(|(key, _)| Value::Str(key)).collect()),
            Value::Bytes => Err(Stop::Unread),
            _ => Err(Stop::Error),
        }
    }

    /// `self + other`.
    pub fn plus(self, other: Value<'a>) -> Eval<Value<'a>> {
        let number = |value: &Value<'_>| {
            matches!(
                value,
                Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Other
            )
        };
        match (self, other) {
            (left, right) if number(&left) && number(&right) => Err(Stop::Unread),
            (Value::Bytes, Value::Bytes) => Ok(Value::Bytes),
            (Value::List(mut left), Value::List(right)) => {
                left.extend(right);
                Ok(Value::List(left))
            }
            (left, right) if left.as_str().is_some() && right.as_str().is_some() => {
                let mut text = left.as_str().unwrap_or_default().to_string();
                text.push_str(right.as_str().unwrap_or_default());
                Ok(Value::Str(text))
            }
            (left, right) if left.is_composable() && right.is_composable() => {
                let parts = |value: Value<'a>| match value {
                    Value::Composed(parts) => parts,
                    other => vec![other],
                };
                let mut all = parts(left);
                all.extend(parts(right));
                Ok(Value::Composed(all))
            }
            _ => Err(Stop::Error),
        }
    }
}

/// One replacement field of a format string, or the text before the next one.
struct Field<'t> {
    literal: String,
    /// `(name, spec, conversion)`.
    field: Option<(&'t str, &'t str, Option<char>)>,
}

/// `_string.formatter_parser`: the parts of a format string; `None` for a `ValueError`.
fn format_parts(text: &str) -> Option<Vec<Field<'_>>> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        let mut brace = None;
        while at < bytes.len() {
            let c = bytes[at];
            at += 1;
            if c == b'{' || c == b'}' {
                brace = Some(c);
                break;
            }
        }
        let at_end = at >= bytes.len();
        let mut end = at;
        let mut markup = brace.is_some();
        match brace {
            Some(b'}') if at_end || bytes[at] != b'}' => return None,
            Some(b'{') if at_end => return None,
            Some(c) => {
                if bytes[at] == c {
                    at += 1;
                    markup = false;
                } else {
                    end -= 1;
                }
            }
            None => {}
        }
        let literal = text[start..end].to_string();
        if !markup {
            parts.push(Field {
                literal,
                field: None,
            });
            continue;
        }
        let name_start = at;
        let mut c = 0;
        while at < bytes.len() {
            c = bytes[at];
            at += 1;
            match c {
                b'{' => return None,
                b'[' => {
                    while at < bytes.len() && bytes[at] != b']' {
                        at += 1;
                    }
                }
                b'}' | b':' | b'!' => break,
                _ => {}
            }
        }
        let name = text.get(name_start..at.checked_sub(1)?)?;
        let mut conversion = None;
        let mut spec = "";
        if c == b'!' || c == b':' {
            let mut closed = false;
            if c == b'!' {
                let found = text.get(at..)?.chars().next()?;
                conversion = Some(found);
                at += found.len_utf8();
                if at < bytes.len() {
                    c = bytes[at];
                    at += 1;
                    if c == b'}' {
                        closed = true;
                    } else if c != b':' {
                        return None;
                    }
                }
            }
            if !closed {
                let spec_start = at;
                let mut count = 1;
                loop {
                    let c = *bytes.get(at)?;
                    at += 1;
                    match c {
                        b'{' => count += 1,
                        b'}' => {
                            count -= 1;
                            if count == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                spec = text.get(spec_start..at - 1)?;
            }
        } else if c != b'}' {
            return None;
        }
        parts.push(Field {
            literal,
            field: Some((name, spec, conversion)),
        });
    }
    Some(parts)
}

/// The arguments of a call, evaluated.
struct Arguments<'a> {
    args: Vec<Value<'a>>,
    kwargs: Vec<(String, Value<'a>)>,
}

impl<'a> Arguments<'a> {
    /// The arguments bound to the parameters `names`, as Python binds them: `None` for one
    /// that is not passed.
    fn bind(self, names: &[&str]) -> Eval<Vec<Option<Value<'a>>>> {
        if self.args.len() > names.len() {
            return Err(Stop::Error);
        }
        let mut bound: Vec<Option<Value<'a>>> = self.args.into_iter().map(Some).collect();
        bound.resize_with(names.len(), || None);
        for (name, value) in self.kwargs {
            let index = names.iter().position(|known| *known == name);
            match index.map(|index| &mut bound[index]) {
                Some(slot @ None) => *slot = Some(value),
                _ => return Err(Stop::Error),
            }
        }
        Ok(bound)
    }

    fn only(self, name: &str) -> Eval<Value<'a>> {
        self.bind(&[name])?.pop().flatten().ok_or(Stop::Error)
    }
}

/// `Composed(seq)`: every part that is no composable becomes a `Literal`.
fn composed(parts: Vec<Value<'_>>) -> Value<'_> {
    Value::Composed(parts)
}

fn construct<'a>(class: &str, arguments: Arguments<'a>) -> Eval<Value<'a>> {
    match class {
        "SQL" => match arguments.only("obj")? {
            value if value.as_str().is_some() => {
                Ok(Value::Sql(value.as_str().unwrap_or_default().to_string()))
            }
            _ => Err(Stop::Error),
        },
        "Identifier" => {
            if !arguments.kwargs.is_empty() || arguments.args.is_empty() {
                return Err(Stop::Error);
            }
            let parts: Option<Vec<String>> = arguments
                .args
                .iter()
                .map(|part| part.as_str().map(str::to_string))
                .collect();
            parts.map(Value::Identifier).ok_or(Stop::Error)
        }
        "Placeholder" => {
            let mut bound = arguments.bind(&["name", "format"])?;
            let format = bound.pop().flatten();
            let name = match bound.pop().flatten() {
                None => String::new(),
                Some(value) => value.as_str().ok_or(Stop::Error)?.to_string(),
            };
            if name.contains(')') {
                return Err(Stop::Error);
            }
            let code = match format {
                None => 's',
                Some(value) => match value.as_str() {
                    Some("s") => 's',
                    Some("b") => 'b',
                    Some("t") => 't',
                    _ => return Err(Stop::Error),
                },
            };
            Ok(Value::Placeholder(name, code))
        }
        _ => Ok(composed(arguments.only("seq")?.items()?)),
    }
}

/// `SQL.format`.
fn sql_format<'a>(template: &str, arguments: &Arguments<'a>) -> Eval<Value<'a>> {
    let mut parts = Vec::new();
    let mut auto = Some(0_usize);
    for part in format_parts(template).ok_or(Stop::Error)? {
        let Some((name, spec, conversion)) = part.field else {
            if !part.literal.is_empty() {
                parts.push(Value::Sql(part.literal));
            }
            continue;
        };
        if !spec.is_empty() || conversion.is_some() {
            return Err(Stop::Error);
        }
        if !part.literal.is_empty() {
            parts.push(Value::Sql(part.literal));
        }
        let index = if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
            if auto.is_some_and(|auto| auto > 0) {
                return Err(Stop::Error);
            }
            auto = None;
            Some(name.parse::<usize>().map_err(|_| Stop::Error)?)
        } else if !name.is_empty() && !name.is_ascii() && name.chars().all(char::is_numeric) {
            // `str.isdigit` holds for digits `int` may or may not read.
            return Err(Stop::Unread);
        } else if name.is_empty() {
            let index = auto.ok_or(Stop::Error)?;
            auto = Some(index + 1);
            Some(index)
        } else {
            None
        };
        let value = match index {
            Some(index) => arguments.args.get(index),
            None => arguments
                .kwargs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value),
        };
        parts.push(value.ok_or(Stop::Error)?.clone());
    }
    Ok(composed(parts))
}

/// `str.format`, for fields without a format spec, a conversion or an attribute.
fn str_format<'a>(template: &str, arguments: &Arguments<'a>) -> Eval<Value<'a>> {
    let mut out = String::new();
    let mut auto = Some(0_usize);
    let mut manual = false;
    for part in format_parts(template).ok_or(Stop::Error)? {
        out.push_str(&part.literal);
        let Some((name, spec, conversion)) = part.field else {
            continue;
        };
        if name.contains(['.', '[']) || !spec.is_empty() || conversion.is_some() {
            return Err(Stop::Unread);
        }
        let value = if name.is_empty() {
            if manual {
                return Err(Stop::Error);
            }
            let index = auto.ok_or(Stop::Error)?;
            auto = Some(index + 1);
            arguments.args.get(index)
        } else if name.chars().all(|c| c.is_ascii_digit()) {
            if auto.is_some_and(|auto| auto > 0) {
                return Err(Stop::Error);
            }
            auto = None;
            manual = true;
            arguments
                .args
                .get(name.parse::<usize>().map_err(|_| Stop::Error)?)
        } else {
            arguments
                .kwargs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
        };
        match value.ok_or(Stop::Error)? {
            Value::None => out.push_str("None"),
            Value::Bool(value) => out.push_str(if *value { "True" } else { "False" }),
            Value::Int(Some(digits)) => out.push_str(digits),
            Value::Float(value) => out.push_str(&crate::unparse::repr_float(*value)),
            value => out.push_str(value.as_str().ok_or(Stop::Unread)?),
        }
    }
    Ok(Value::Str(out))
}

/// `str.join`.
fn str_join<'a>(joiner: &str, arguments: Arguments<'a>) -> Eval<Value<'a>> {
    if !arguments.kwargs.is_empty() || arguments.args.len() != 1 {
        return Err(Stop::Error);
    }
    let items = match arguments.only("iterable")? {
        // Its parts are composables, never strings.
        Value::Composed(parts) if !parts.is_empty() => return Err(Stop::Error),
        other => other.items()?,
    };
    let texts: Option<Vec<&str>> = items.iter().map(Value::as_str).collect();
    Ok(Value::Str(texts.ok_or(Stop::Error)?.join(joiner)))
}

/// `SQL.join`.
fn sql_join<'a>(joiner: &str, arguments: Arguments<'a>) -> Eval<Value<'a>> {
    let items = arguments.only("seq")?.items()?;
    let mut parts = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            parts.push(Value::Sql(joiner.to_string()));
        }
        parts.push(item);
    }
    Ok(composed(parts))
}

/// A function or lambda: where it starts and ends, and its parameters.
struct Function {
    start: usize,
    end: usize,
    params: HashSet<String>,
}

/// `PsycopgScan`: what a walk of a module collects to follow names and imports.
pub struct Scan<'a> {
    source: &'a Source,
    functions: Vec<Function>,
    /// The places that bind each name, with the value a single-name assignment gives it.
    stores: HashMap<&'a str, Vec<(usize, Option<&'a Expr>)>>,
    /// Local names bound by `psycopg` imports, to their dotted target.
    table: HashMap<String, String>,
}

struct Collect<'a, 'b> {
    scan: &'b mut Scan<'a>,
    text: &'a str,
}

fn parameter_names(parameters: &Parameters) -> HashSet<String> {
    parameters
        .iter()
        .map(|parameter| parameter.name().to_string())
        .collect()
}

impl<'a> Visitor<'a> for Collect<'a, '_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::FunctionDef(node) => {
                // Python counts a function from `def` or `async`, not from its decorators.
                let mut start = node.start().to_usize();
                if !node.decorator_list.is_empty() {
                    let trim = |text: &'a str| {
                        text.trim_end_matches(|c: char| c.is_whitespace() || c == '\\')
                    };
                    let mut before = trim(&self.text[..node.name.start().to_usize()]);
                    before = before.strip_suffix("def").unwrap_or(before);
                    if node.is_async {
                        before = trim(before);
                        before = before.strip_suffix("async").unwrap_or(before);
                    }
                    start = before.len();
                }
                self.scan.functions.push(Function {
                    start,
                    end: node.end().to_usize(),
                    params: parameter_names(&node.parameters),
                });
            }
            Stmt::Assign(node) => {
                if let [Expr::Name(target)] = node.targets.as_slice() {
                    self.store(target, Some(&node.value));
                }
            }
            Stmt::AnnAssign(node) => {
                if let (Expr::Name(target), Some(value)) = (node.target.as_ref(), &node.value) {
                    self.store(target, Some(value));
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        match expr {
            Expr::Name(name) if name.ctx == ExprContext::Store => self.store(name, None),
            Expr::Lambda(node) => self.scan.functions.push(Function {
                start: node.start().to_usize(),
                end: node.end().to_usize(),
                params: node
                    .parameters
                    .as_deref()
                    .map(parameter_names)
                    .unwrap_or_default(),
            }),
            // `parts.append(x)` and `parts[0] = x` change the value bound to `parts`.
            Expr::Call(call) => {
                if let Expr::Attribute(func) = call.func.as_ref() {
                    if MUTATORS.contains(&func.attr.as_str()) {
                        if let Expr::Name(name) = func.value.as_ref() {
                            self.store(name, None);
                        }
                    }
                }
            }
            Expr::Subscript(node) if node.ctx != ExprContext::Load => {
                if let Expr::Name(name) = node.value.as_ref() {
                    self.store(name, None);
                }
            }
            _ => {}
        }
        visitor::walk_expr(self, expr);
    }
}

impl<'a> Collect<'a, '_> {
    /// A place that binds `name`; the value of its assignment is kept when one is known.
    fn store(&mut self, name: &'a ExprName, value: Option<&'a Expr>) {
        let at = name.start().to_usize();
        let known = self.scan.stores.entry(name.id.as_str()).or_default();
        match known.iter_mut().find(|(place, _)| *place == at) {
            Some(found) => found.1 = found.1.or(value),
            None => known.push((at, value)),
        }
    }
}

impl<'a> Scan<'a> {
    /// `ParsedSource.psycopg`: bindings are collected only when the text names `psycopg`.
    pub fn new(source: &'a Source) -> Self {
        let mut table = HashMap::new();
        for import in &source.entries {
            match &import.module {
                None => {
                    if import.original.split('.').next() == Some("psycopg") {
                        if import.local == import.original {
                            table.insert("psycopg".to_string(), "psycopg".to_string());
                        } else {
                            table.insert(import.local.clone(), import.original.clone());
                        }
                    }
                }
                Some(module) => {
                    if import.level == 0 && module.split('.').next() == Some("psycopg") {
                        table.insert(
                            import.local.clone(),
                            format!("{module}.{}", import.original),
                        );
                    }
                }
            }
        }
        let mut scan = Scan {
            source,
            functions: Vec::new(),
            stores: HashMap::new(),
            table,
        };
        if source.text.contains("psycopg") {
            let mut collect = Collect {
                scan: &mut scan,
                text: &source.text,
            };
            collect.visit_body(source.body());
        }
        scan
    }

    /// `PsycopgScan.binding`: the value of the single assignment `name` refers to.
    fn binding(&self, name: &ExprName) -> Option<&'a Expr> {
        let at = name.start().to_usize();
        let mut enclosing: Vec<&Function> = self
            .functions
            .iter()
            .filter(|function| function.start <= at && at < function.end)
            .collect();
        enclosing.sort_by_key(|function| std::cmp::Reverse(function.start));
        let stores = self.stores.get(name.id.as_str());
        for scope in enclosing.iter().map(|scope| Some(*scope)).chain([None]) {
            if scope.is_some_and(|scope| scope.params.contains(name.id.as_str())) {
                return None;
            }
            let found: Vec<&(usize, Option<&'a Expr>)> = stores
                .into_iter()
                .flatten()
                .filter(|(place, _)| {
                    let owner = self.owner(*place);
                    match (owner, scope) {
                        (Some(owner), Some(scope)) => std::ptr::eq(owner, scope),
                        (None, None) => true,
                        _ => false,
                    }
                })
                .collect();
            match found.as_slice() {
                [] => {}
                [only] => return only.1,
                _ => return None,
            }
        }
        None
    }

    /// The innermost function containing `at`.
    fn owner(&self, at: usize) -> Option<&Function> {
        let mut best: Option<&Function> = None;
        for function in &self.functions {
            if function.start <= at
                && at < function.end
                && best.is_none_or(|best| best.start <= function.start)
            {
                best = Some(function);
            }
        }
        best
    }

    fn is_parameter(&self, name: &ExprName) -> bool {
        let at = name.start().to_usize();
        self.functions.iter().any(|function| {
            function.start <= at && at < function.end && function.params.contains(name.id.as_str())
        })
    }

    /// The `psycopg.sql` class `func` names, through any import form.
    fn symbol(&self, func: &Expr) -> Option<&'static str> {
        let dotted = dotted(func)?;
        let (head, rest) = dotted.split_once('.').unwrap_or((&dotted, ""));
        let target = self.table.get(head)?;
        let full = if rest.is_empty() {
            target.clone()
        } else {
            format!("{target}.{rest}")
        };
        let (module, name) = full.rsplit_once('.')?;
        if module != "psycopg.sql" {
            return None;
        }
        CLASSES.iter().find(|class| **class == name).copied()
    }
}

/// `ParsedSource.constants`: module-level names to the value of their one assignment.
pub fn constants(source: &Source) -> Dict<Option<&Expr>> {
    fn bind<'e>(found: &mut Dict<Option<&'e Expr>>, name: &str, value: Option<&'e Expr>) {
        // A name bound again has no single value.
        let value = if found.get(name).is_some() {
            None
        } else {
            value
        };
        found.insert(name, value);
    }
    let mut found: Dict<Option<&Expr>> = Dict::new();
    for stmt in source.body() {
        match stmt {
            Stmt::Assign(node) => {
                if node.targets.iter().all(Expr::is_name_expr) {
                    let mut seen: Vec<&str> = Vec::new();
                    for target in &node.targets {
                        if let Expr::Name(target) = target {
                            if !seen.contains(&target.id.as_str()) {
                                seen.push(target.id.as_str());
                            }
                        }
                    }
                    for name in seen {
                        bind(&mut found, name, Some(&node.value));
                    }
                }
            }
            Stmt::AnnAssign(node) => {
                if let Expr::Name(target) = node.target.as_ref() {
                    bind(&mut found, target.id.as_str(), node.value.as_deref());
                }
            }
            Stmt::AugAssign(node) => {
                if let Expr::Name(target) = node.target.as_ref() {
                    bind(&mut found, target.id.as_str(), None);
                }
            }
            Stmt::FunctionDef(node) => bind(&mut found, node.name.as_str(), None),
            Stmt::ClassDef(node) => bind(&mut found, node.name.as_str(), None),
            Stmt::Import(node) => {
                let mut seen: Vec<&str> = Vec::new();
                for alias in &node.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let head = local.split('.').next().unwrap_or(local);
                    if !seen.contains(&head) {
                        seen.push(head);
                    }
                }
                for name in seen {
                    bind(&mut found, name, None);
                }
            }
            Stmt::ImportFrom(node) => {
                let mut seen: Vec<&str> = Vec::new();
                for alias in &node.names {
                    let local = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
                    let head = local.split('.').next().unwrap_or(local);
                    if !seen.contains(&head) {
                        seen.push(head);
                    }
                }
                for name in seen {
                    bind(&mut found, name, None);
                }
            }
            _ => {}
        }
    }
    found
}

/// `resolve(source, dotted)`: the module-level value of a name `source` imports from
/// another module, with the source it is an expression of.
pub type Resolver<'a, 'r> = &'r dyn Fn(&Source, &str) -> Option<(&'a Source, &'a Expr)>;
/// `lookup(name)`: the value of a name on the path a walk follows.
pub type Lookup<'a, 'r> = &'r dyn Fn(&ExprName) -> Eval<Value<'a>>;

/// `_Run`: one evaluation of an expression.
pub struct Run<'a, 'r> {
    scan: &'r Scan<'a>,
    resolve: Option<Resolver<'a, 'r>>,
    lookup: Option<Lookup<'a, 'r>>,
    /// Whether a `psycopg.sql` class was called.
    pub seen: bool,
    depth: usize,
}

impl<'a, 'r> Run<'a, 'r> {
    pub fn new(
        scan: &'r Scan<'a>,
        resolve: Option<Resolver<'a, 'r>>,
        lookup: Option<Lookup<'a, 'r>>,
    ) -> Self {
        Run {
            scan,
            resolve,
            lookup,
            seen: false,
            depth: 0,
        }
    }

    /// `_Run._imported`: the value of a name or `module.NAME` bound in another module.
    fn imported(&mut self, node: &Expr) -> Eval<Value<'a>> {
        let resolve = self.resolve.ok_or(Stop::Dynamic)?;
        let dotted = dotted(node).ok_or(Stop::Dynamic)?;
        if let Expr::Name(name) = node {
            if self.scan.stores.contains_key(name.id.as_str()) || self.scan.is_parameter(name) {
                return Err(Stop::Dynamic);
            }
        }
        let (source, expr) = resolve(self.scan.source, &dotted).ok_or(Stop::Dynamic)?;
        let scan = Scan::new(source);
        let mut inner = Run::new(&scan, self.resolve, None);
        inner.depth = self.depth;
        let found = inner.value(expr);
        self.seen = self.seen || inner.seen;
        found
    }

    /// `_Run.value`: the value of `node`, its names read with `lookup` when one is given.
    pub fn value(&mut self, node: &Expr) -> Eval<Value<'a>> {
        self.depth += 1;
        let found = if self.depth > 64 {
            Err(Stop::Dynamic)
        } else {
            self.evaluate(node)
        };
        self.depth -= 1;
        found
    }

    fn evaluate(&mut self, node: &Expr) -> Eval<Value<'a>> {
        let text = &*self.scan.source.text;
        match node {
            Expr::StringLiteral(literal) => Ok(Value::Str(crate::parse::text(literal, text))),
            Expr::BytesLiteral(_) => Ok(Value::Bytes),
            Expr::NoneLiteral(_) => Ok(Value::None),
            Expr::BooleanLiteral(literal) => Ok(Value::Bool(literal.value)),
            Expr::EllipsisLiteral(_) => Ok(Value::Other),
            Expr::NumberLiteral(literal) => Ok(match &literal.value {
                Number::Int(value) => Value::Int(value.as_u64().map(|value| value.to_string())),
                Number::Float(value) => Value::Float(*value),
                Number::Complex { .. } => Value::Other,
            }),
            Expr::Name(name) => {
                if let Some(lookup) = self.lookup {
                    return lookup(name);
                }
                match self.scan.binding(name) {
                    Some(target) => self.value(target),
                    None => self.imported(node),
                }
            }
            Expr::Attribute(_) => self.imported(node),
            Expr::List(_) | Expr::Tuple(_) => {
                let items = match node {
                    Expr::List(list) => &list.elts,
                    Expr::Tuple(tuple) => &tuple.elts,
                    _ => return Err(Stop::Dynamic),
                };
                if items.iter().any(Expr::is_starred_expr) {
                    return Err(Stop::Dynamic);
                }
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.value(item)?);
                }
                Ok(Value::List(values))
            }
            Expr::BinOp(node) if node.op == Operator::Add => {
                let left = self.value(&node.left)?;
                let right = self.value(&node.right)?;
                left.plus(right)
            }
            Expr::Call(call) => {
                if let Some(class) = self.scan.symbol(&call.func) {
                    self.seen = true;
                    if class == "Literal" {
                        return Ok(Value::Sql("NULL".to_string()));
                    }
                    let arguments = self.arguments(call)?;
                    return construct(class, arguments);
                }
                let Expr::Attribute(func) = call.func.as_ref() else {
                    return Err(Stop::Dynamic);
                };
                let method = func.attr.as_str();
                if method != "format" && method != "join" {
                    return Err(Stop::Dynamic);
                }
                let receiver = self.value(&func.value)?;
                let is_sql = matches!(receiver, Value::Sql(_));
                let template = match &receiver {
                    Value::Sql(text) => text.clone(),
                    other => other.as_str().ok_or(Stop::Error)?.to_string(),
                };
                let arguments = self.arguments(call)?;
                match (is_sql, method) {
                    (true, "format") => sql_format(&template, &arguments),
                    (true, _) => sql_join(&template, arguments),
                    (false, "format") => str_format(&template, &arguments),
                    (false, _) => str_join(&template, arguments),
                }
            }
            _ => Err(Stop::Dynamic),
        }
    }

    fn arguments(&mut self, call: &ruff_python_ast::ExprCall) -> Eval<Arguments<'a>> {
        let arguments = &call.arguments;
        if arguments.args.iter().any(Expr::is_starred_expr)
            || arguments
                .keywords
                .iter()
                .any(|keyword| keyword.arg.is_none())
        {
            return Err(Stop::Dynamic);
        }
        // Python evaluates the positional arguments, then the keywords, each in order.
        let mut args = Vec::new();
        for arg in &arguments.args {
            args.push(self.value(arg)?);
        }
        let mut kwargs = Vec::new();
        for keyword in &arguments.keywords {
            let value = self.value(&keyword.value)?;
            if let Some(name) = &keyword.arg {
                kwargs.push((name.to_string(), value));
            }
        }
        Ok(Arguments { args, kwargs })
    }
}

/// What `PsycopgScan.render` gives for an expression.
#[derive(Debug, PartialEq, Eq)]
pub enum Rendered {
    /// The SQL of a composition.
    Text(String),
    /// A composition with a dynamic part.
    Unchecked,
    /// No psycopg composition.
    None,
}

/// `PsycopgScan.render`: the SQL of a psycopg composition; `None` where Python computes
/// something this model does not.
pub fn render<'a>(
    scan: &Scan<'a>,
    expr: &Expr,
    resolve: Option<Resolver<'a, '_>>,
) -> Option<Rendered> {
    let mut run = Run::new(scan, resolve, None);
    let rendered = run.value(expr).and_then(|value| {
        if value.is_composable() {
            value.as_string().map(Some)
        } else {
            Ok(None)
        }
    });
    match rendered {
        Ok(Some(text)) => Some(Rendered::Text(text)),
        Ok(None) | Err(Stop::Error) => Some(Rendered::None),
        Err(Stop::Dynamic) if run.seen => Some(Rendered::Unchecked),
        Err(Stop::Dynamic) => Some(Rendered::None),
        Err(Stop::Unread) => None,
    }
}
