//! The calls that run a query: calls of the helpers a module defines (functions with a
//! parameter marked `Annotated[str, SQL]`), as `sqlty._helpers.find_helper_calls` finds
//! them, driver calls ([`crate::drivers`]) and `--sql` t-strings ([`crate::templates`]).

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{Expr, ExprCall, Parameters, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::helpers::{mentions, receiver_class, region};
use crate::scopes::{dotted, Context, Key, Lines, Node, Param, Scan, Scope};
use crate::types::info::Dict;
use crate::types::symbols::Source;

const TYPE_FORMS: [&str; 3] = ["type", "Type", "TypeForm"];
const PARAMS_ARGUMENT: [&str; 3] = ["params", "parameters", "args"];
const PARAMS_VARIADIC: [&str; 3] = ["args", "params", "parameters"];
const PARAMS_KEYWORDS: [&str; 2] = ["params", "parameters"];
const TYPING_MODULES: [&str; 2] = ["typing", "typing_extensions"];

/// Whether `query` opens with the `--sql` marker, which the engine reads by itself.
pub fn is_marked(query: &str) -> bool {
    static MARKER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[\s\x1c-\x1f]*--[\s\x1c-\x1f]?sql(?:[\s\x1c-\x1f]|$)")
            .expect("a valid pattern")
    });
    MARKER.is_match(query)
}

/// How a method is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Function,
    Static,
    Class,
    Instance,
}

/// A function with a marked query parameter, see `sqlty._helpers.Helper`.
#[derive(Clone, Debug)]
pub struct Helper {
    pub name: String,
    pub positional: Vec<String>,
    pub keywords: HashSet<String>,
    pub query: String,
    pub row: Option<String>,
    pub kind: Kind,
    pub params: Option<String>,
    pub rest: Option<String>,
    pub named: Option<String>,
    pub configured: bool,
    pub database: Option<String>,
}

/// A call that runs a query, see `sqlty._helpers.HelperCall`.
#[derive(Clone, Debug)]
pub struct Call<'a> {
    pub line: usize,
    /// `None` for a query that is not a string literal.
    pub query: Option<String>,
    /// The row type the call names.
    pub row: Option<&'a Expr>,
    /// The arguments for the placeholders, `None` when not spelled out.
    pub params: Option<Vec<Param>>,
    pub end_line: Option<usize>,
    pub driver: Option<&'static str>,
    pub single: Option<String>,
    pub dynamic: bool,
    pub reason: Option<String>,
    pub configured: bool,
    /// The byte offsets of the argument the query comes from.
    pub query_span: Option<(usize, usize)>,
}

impl Call<'_> {
    /// The lines of the call when its strings need no `SQL[T]`.
    pub fn typed_lines(&self) -> std::ops::RangeInclusive<usize> {
        if self.row.is_some() || self.configured {
            self.line..=self.end_line.unwrap_or(self.line)
        } else {
            #[expect(clippy::reversed_empty_ranges, reason = "the empty range")]
            {
                1..=0
            }
        }
    }
}

/// `ParsedSource.marker_names`: the local spellings of `Annotated` and of `sqlty.SQL`.
fn marker_names(source: &Source) -> (HashSet<String>, HashSet<String>) {
    let (mut annotated, mut marker) = (HashSet::new(), HashSet::new());
    for import in &source.entries {
        match &import.module {
            Some(_) if import.level != 0 => {}
            Some(module) => {
                if TYPING_MODULES.contains(&module.as_str()) && import.original == "Annotated" {
                    annotated.insert(import.local.clone());
                } else if module == "sqlty" && import.original == "SQL" {
                    marker.insert(import.local.clone());
                }
            }
            None => {
                if TYPING_MODULES.contains(&import.original.as_str()) {
                    annotated.insert(format!("{}.Annotated", import.local));
                } else if import.original == "sqlty" {
                    marker.insert(format!("{}.SQL", import.local));
                }
            }
        }
    }
    (annotated, marker)
}

/// `_expr`: the annotation, a quoted one parsed.
fn with_annotation<T>(annotation: Option<&Expr>, read: impl FnOnce(&Expr) -> T) -> Option<T> {
    match annotation? {
        Expr::StringLiteral(text) => {
            let parsed = crate::parse::expression(text.value.to_str())?;
            Some(read(&parsed.syntax().body))
        }
        annotation => Some(read(annotation)),
    }
}

struct Marked<'s> {
    annotated: &'s HashSet<String>,
    marker: &'s HashSet<String>,
    found: bool,
}

impl<'a> Visitor<'a> for Marked<'_> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Subscript(sub) = expr {
            if let (Some(name), Expr::Tuple(items)) = (dotted(&sub.value), sub.slice.as_ref()) {
                if self.annotated.contains(&name)
                    && items
                        .elts
                        .iter()
                        .skip(1)
                        .any(|meta| dotted(meta).is_some_and(|meta| self.marker.contains(&meta)))
                {
                    self.found = true;
                }
            }
        }
        walk_expr(self, expr);
    }
}

/// `_is_marked`: whether an annotation is `Annotated[..., SQL]`, at any depth.
fn annotation_is_marked(
    annotation: Option<&Expr>,
    annotated: &HashSet<String>,
    marker: &HashSet<String>,
) -> bool {
    with_annotation(annotation, |annotation| {
        let mut visitor = Marked {
            annotated,
            marker,
            found: false,
        };
        visitor.visit_expr(annotation);
        visitor.found
    })
    .unwrap_or(false)
}

struct Names<'a>(HashSet<&'a str>);

impl<'a> Visitor<'a> for Names<'a> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            self.0.insert(name.id.as_str());
        }
        walk_expr(self, expr);
    }
}

struct TypeForm<'s> {
    returned: &'s HashSet<String>,
    found: bool,
}

impl<'a> Visitor<'a> for TypeForm<'_> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Subscript(sub) = expr {
            if let (Some(name), Expr::Name(item)) = (dotted(&sub.value), sub.slice.as_ref()) {
                let last = name.rsplit('.').next().unwrap_or(&name);
                if TYPE_FORMS.contains(&last) && self.returned.contains(item.id.as_str()) {
                    self.found = true;
                }
            }
        }
        walk_expr(self, expr);
    }
}

fn named_parameters(parameters: &Parameters) -> impl Iterator<Item = &ruff_python_ast::Parameter> {
    parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .chain(&parameters.kwonlyargs)
        .map(|parameter| &parameter.parameter)
}

/// `_row_param`: the first parameter annotated `type[T]` with `T` in the return type.
fn row_param(function: &StmtFunctionDef) -> Option<String> {
    let returned: HashSet<String> = with_annotation(function.returns.as_deref(), |returns| {
        let mut names = Names(HashSet::new());
        names.visit_expr(returns);
        names.0.into_iter().map(str::to_string).collect()
    })?;
    named_parameters(&function.parameters)
        .find(|parameter| {
            with_annotation(parameter.annotation.as_deref(), |annotation| {
                let mut visitor = TypeForm {
                    returned: &returned,
                    found: false,
                };
                visitor.visit_expr(annotation);
                visitor.found
            })
            .unwrap_or(false)
        })
        .map(|parameter| parameter.name.to_string())
}

fn decorators(function: &StmtFunctionDef) -> impl Iterator<Item = Option<String>> + '_ {
    function
        .decorator_list
        .iter()
        .map(|decorator| dotted(&decorator.expression))
}

/// `_kind`: how a method is called, by its decorators.
fn method_kind(function: &StmtFunctionDef) -> Kind {
    let names: Vec<Option<String>> = decorators(function).collect();
    let has = |name: &str| names.iter().any(|found| found.as_deref() == Some(name));
    if has("staticmethod") {
        Kind::Static
    } else if has("classmethod") {
        Kind::Class
    } else {
        Kind::Instance
    }
}

/// `_helper`: the helper `function` defines, its query parameter being `query`.
fn helper(
    name: String,
    function: &StmtFunctionDef,
    kind: Kind,
    query: String,
    row: Option<String>,
) -> Helper {
    let parameters = &function.parameters;
    let name_of =
        |parameter: &ruff_python_ast::ParameterWithDefault| parameter.parameter.name.to_string();
    let mut positional: Vec<String> = parameters
        .posonlyargs
        .iter()
        .chain(&parameters.args)
        .map(name_of)
        .collect();
    let mut keywords: Vec<String> = parameters
        .args
        .iter()
        .chain(&parameters.kwonlyargs)
        .map(name_of)
        .collect();
    let names: Vec<String> = named_parameters(parameters)
        .map(|parameter| parameter.name.to_string())
        .collect();
    if matches!(kind, Kind::Instance | Kind::Class) && !positional.is_empty() {
        // The receiver is the first positional parameter, itself and not one of its name.
        if parameters.posonlyargs.is_empty() {
            keywords.remove(0);
        }
        positional.remove(0);
    }
    let variadic = |parameter: Option<&ruff_python_ast::Parameter>, known: &[&str]| {
        parameter
            .map(|parameter| parameter.name.to_string())
            .filter(|name| known.contains(&name.as_str()))
    };
    Helper {
        name,
        positional,
        keywords: keywords.into_iter().collect(),
        params: names
            .iter()
            .find(|name| {
                PARAMS_ARGUMENT.contains(&name.as_str())
                    && **name != query
                    && Some(*name) != row.as_ref()
            })
            .cloned(),
        rest: variadic(parameters.vararg.as_deref(), &PARAMS_VARIADIC),
        named: variadic(parameters.kwarg.as_deref(), &PARAMS_KEYWORDS),
        query,
        row,
        kind,
        configured: false,
        database: None,
    }
}

/// `index_functions`: the functions and methods of `source` with a marked query
/// parameter, by name (`Class.method`).
pub fn index_functions(source: &Source) -> Dict<Helper> {
    let (annotated, marker) = marker_names(source);
    let mut found: Dict<((bool, bool), Helper)> = Dict::new();
    if annotated.is_empty() || marker.is_empty() {
        return Dict::new();
    }
    let functions = source.body().iter().filter_map(|stmt| match stmt {
        Stmt::FunctionDef(function) => Some((function.name.to_string(), function, Kind::Function)),
        _ => None,
    });
    let classes = source.classes();
    let methods = classes.iter().flat_map(|class| {
        class.body.iter().filter_map(move |stmt| match stmt {
            Stmt::FunctionDef(function) => Some((
                format!("{}.{}", class.name, function.name),
                function,
                method_kind(function),
            )),
            _ => None,
        })
    });
    for (name, function, kind) in functions.chain(methods) {
        let Some(query) = named_parameters(&function.parameters)
            .find(|parameter| {
                annotation_is_marked(parameter.annotation.as_deref(), &annotated, &marker)
            })
            .map(|parameter| parameter.name.to_string())
        else {
            continue;
        };
        let row = row_param(function);
        let overload = decorators(function).any(|decorator| {
            decorator.is_some_and(|name| name.rsplit('.').next() == Some("overload"))
        });
        let rank = (row.is_none(), overload);
        if found.get(&name).is_none_or(|(known, _)| rank < *known) {
            found.insert(
                &name,
                (rank, helper(name.clone(), function, kind, query, row)),
            );
        }
    }
    let mut out = Dict::new();
    for (name, (_, helper)) in found.iter() {
        out.insert(name, helper.clone());
    }
    out
}

/// `_argument`: the argument of `call` bound to `param`.
fn argument<'a>(helper: &Helper, call: &'a ExprCall, param: Option<&str>) -> Option<&'a Expr> {
    let param = param?;
    if helper.keywords.contains(param) {
        for keyword in &call.arguments.keywords {
            if keyword
                .arg
                .as_ref()
                .is_some_and(|name| name.as_str() == param)
            {
                return Some(&keyword.value);
            }
        }
    }
    let index = helper.positional.iter().position(|name| name == param)?;
    let args = &call.arguments.args;
    if args.iter().take(index + 1).any(Expr::is_starred_expr) {
        return None;
    }
    args.get(index)
}

/// `_call_params`: the arguments `call` passes for the placeholders of its query.
fn call_params(
    helper: &Helper,
    call: &ExprCall,
    stack: &[Scope],
    ctx: &Context<'_, '_>,
) -> Option<Vec<Param>> {
    if helper.params.is_some() {
        let arg = argument(helper, call, helper.params.as_deref())?;
        return ctx.literal_params(arg, stack);
    }
    let args = &call.arguments.args;
    if helper.rest.is_some() {
        if args.iter().any(Expr::is_starred_expr) {
            return None;
        }
        return Some(
            args.iter()
                .skip(helper.positional.len())
                .enumerate()
                .map(|(i, arg)| Param {
                    key: Key::Index(i + 1),
                    kind: ctx.arg_type(arg, stack),
                })
                .collect(),
        );
    }
    if helper.named.is_some() {
        let keywords = &call.arguments.keywords;
        if keywords.iter().any(|keyword| keyword.arg.is_none()) {
            return None;
        }
        return Some(
            keywords
                .iter()
                .filter_map(|keyword| {
                    let name = keyword.arg.as_ref()?.as_str();
                    (!helper.keywords.contains(name)).then(|| Param {
                        key: Key::Name(name.to_string()),
                        kind: ctx.arg_type(&keyword.value, stack),
                    })
                })
                .collect(),
        );
    }
    None
}

/// The calls of the helpers `source` defines, by line; `None` where a query is one Rust
/// does not read yet: a psycopg composition, or a name that may stand for a query.
pub fn helper_calls(source: &Source) -> Option<Vec<Call<'_>>> {
    let text = &*source.text;
    if !(text.contains("Annotated") && text.contains("sqlty") && text.contains("SQL")) {
        return Some(Vec::new());
    }
    let own = index_functions(source);
    if own.is_empty() {
        return Some(Vec::new());
    }
    let mut names: Vec<String> = own
        .iter()
        .map(|(name, _)| name.rsplit('.').next().unwrap_or(name).to_string())
        .collect();
    names.sort();
    names.dedup();
    let pattern = mentions(&names)?;
    let scan = Scan::new(source.body(), |call: &ExprCall| match call.func.as_ref() {
        Expr::Name(_) => true,
        Expr::Attribute(attr) => dotted(&attr.value).is_some(),
        _ => false,
    });
    let ctx = Context {
        imported: &source.imports,
        scan: &scan,
    };
    let lines = Lines::new(text);
    let (annotated, marker) = marker_names(source);
    let mut wanted: Vec<Option<bool>> = vec![None; scan.scopes.len()];
    let mut calls = Vec::new();
    for (call, stack) in &scan.calls {
        if let Some(&scope) = stack.last() {
            let mentioned = *wanted[scope].get_or_insert_with(|| {
                let (start, end) = region(scan.scopes[scope], &lines, text.len());
                pattern.is_match(&text[start..end])
            });
            if !mentioned {
                continue;
            }
        }
        let helper = match call.func.as_ref() {
            Expr::Name(name) => own.get(name.id.as_str()),
            Expr::Attribute(attr) => {
                let prefix = dotted(&attr.value).unwrap_or_default();
                let method = |class: &str| {
                    (!class.contains('.'))
                        .then(|| own.get(&format!("{class}.{}", attr.attr)))
                        .flatten()
                };
                match method(&prefix) {
                    Some(found) => (found.kind != Kind::Instance).then_some(found),
                    None => {
                        receiver_class(&attr.value, stack, &ctx).and_then(|class| method(&class))
                    }
                }
            }
            _ => None,
        };
        let Some(helper) = helper else {
            continue;
        };
        let Some(query) = argument(helper, call, Some(&helper.query)) else {
            continue;
        };
        let literal = match query {
            Expr::StringLiteral(literal) => Some(crate::parse::text(literal, text)),
            _ => None,
        };
        if literal.is_none() {
            if forwarded(query, stack, &scan, &annotated, &marker) {
                continue;
            }
            if text.contains("psycopg") || matches!(query, Expr::Name(_) | Expr::Attribute(_)) {
                return None;
            }
        }
        calls.push(Call {
            line: lines.line(call.start().to_usize()),
            dynamic: literal.is_none(),
            query: literal,
            row: argument(helper, call, helper.row.as_deref()),
            params: call_params(helper, call, stack, &ctx),
            end_line: Some(lines.line(call.end().to_usize().saturating_sub(1))),
            driver: None,
            single: None,
            reason: None,
            configured: helper.configured,
            query_span: Some((query.start().to_usize(), query.end().to_usize())),
        });
    }
    calls.sort_by_key(|call| (call.line, call.query_span));
    Some(calls)
}

/// Whether `query` is the marked query parameter of a function around the call.
fn forwarded(
    query: &Expr,
    stack: &[Scope],
    scan: &Scan<'_>,
    annotated: &HashSet<String>,
    marker: &HashSet<String>,
) -> bool {
    let Expr::Name(query) = query else {
        return false;
    };
    for &scope in stack.iter().rev() {
        let Node::Function(function) = scan.scopes[scope] else {
            continue;
        };
        if let Some(parameter) = named_parameters(&function.parameters)
            .find(|parameter| parameter.name.as_str() == query.id.as_str())
        {
            return annotation_is_marked(parameter.annotation.as_deref(), annotated, marker);
        }
    }
    false
}
