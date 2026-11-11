//! The calls that run a query: calls of the helpers a module defines (functions with a
//! parameter marked `Annotated[str, SQL]`), as `sqlty._helpers.find_helper_calls` finds
//! them, driver calls ([`crate::drivers`]) and `--sql` t-strings ([`crate::templates`]).

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::index::{imports_of, module_of, HelperIndex};
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{Expr, ExprCall, Parameters, Stmt, StmtFunctionDef};
use ruff_text_size::Ranged;

use crate::compose::{self, Declared, Rendered};
use crate::helpers::{mentions, receiver_class, region};
use crate::scopes::{dotted, Context, Key, Lines, Node, Param, Scan, Scope};
use crate::types::info::Dict;
use crate::types::symbols::Source;
use crate::variants::{query_variants, Found, Variant};

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
    /// Why a query built in branches is not checked.
    pub reason: Option<String>,
    /// Every query a call built in branches can run, `query` being the first.
    pub variants: Vec<Variant<'a>>,
    /// The `SQL[R, P]("...")` the query is that of.
    pub declared: Option<Declared<'a>>,
    pub configured: bool,
    /// The byte offsets of the argument the query comes from.
    pub query_span: Option<(usize, usize)>,
    /// Whether that argument is a string literal.
    pub literal: bool,
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
pub(crate) fn marker_names(source: &Source) -> (HashSet<String>, HashSet<String>) {
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
pub(crate) fn helper(
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

/// `_definitions`: the module-level functions and the methods of `source`, methods as
/// `Class.name`.
pub(crate) fn definitions(source: &Source) -> Vec<(String, &StmtFunctionDef, Kind)> {
    let functions = source.body().iter().filter_map(|stmt| match stmt {
        Stmt::FunctionDef(function) => Some((function.name.to_string(), function, Kind::Function)),
        _ => None,
    });
    let methods = source.classes().into_iter().flat_map(|class| {
        class.body.iter().filter_map(move |stmt| match stmt {
            Stmt::FunctionDef(function) => Some((
                format!("{}.{}", class.name, function.name),
                function,
                method_kind(function),
            )),
            _ => None,
        })
    });
    functions.chain(methods).collect()
}

/// `_rank`: the sort key among definitions of one name: complete first, then not `@overload`.
pub(crate) fn rank(function: &StmtFunctionDef, complete: bool) -> (bool, bool) {
    let overload = decorators(function)
        .any(|decorator| decorator.is_some_and(|name| name.rsplit('.').next() == Some("overload")));
    (!complete, overload)
}

/// `_parameters`: the names of the parameters a call can name.
pub(crate) fn parameter_names(function: &StmtFunctionDef) -> HashSet<String> {
    named_parameters(&function.parameters)
        .map(|parameter| parameter.name.to_string())
        .collect()
}

/// `index_functions`: the functions and methods of `source` with a marked query
/// parameter, by name (`Class.method`).
pub fn index_functions(source: &Source) -> Dict<Helper> {
    let (annotated, marker) = marker_names(source);
    let mut found: Dict<((bool, bool), Helper)> = Dict::new();
    if annotated.is_empty() || marker.is_empty() {
        return Dict::new();
    }
    for (name, function, kind) in definitions(source) {
        let Some(query) = named_parameters(&function.parameters)
            .find(|parameter| {
                annotation_is_marked(parameter.annotation.as_deref(), &annotated, &marker)
            })
            .map(|parameter| parameter.name.to_string())
        else {
            continue;
        };
        let row = row_param(function);
        let rank = rank(function, row.is_some());
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

/// `find_helper_calls`: the calls of the helpers `source` defines or imports from the
/// modules of `helpers`, by line. A query built in a way that is not modelled (see
/// [`compose`]) is left unchecked.
pub fn helper_calls<'a>(
    source: &'a Source,
    helpers: Option<&'a HelperIndex>,
    module: Option<&str>,
    variants: Option<usize>,
) -> Vec<Call<'a>> {
    let text = &*source.text;
    let local = text.contains("Annotated") && text.contains("sqlty") && text.contains("SQL");
    let imported =
        helpers.is_some_and(|helpers| !helpers.modules.is_empty() && helpers.mentions(text));
    if !(local || imported) {
        return Vec::new();
    }
    let own = if local {
        index_functions(source)
    } else {
        Dict::new()
    };
    if own.is_empty() && !imported {
        return Vec::new();
    }
    let (functions, modules) = if imported {
        imports_of(&source.entries, module.or(source.module.as_deref()))
    } else {
        Default::default()
    };
    let mut names: HashSet<String> = own
        .iter()
        .map(|(name, _)| name.rsplit('.').next().unwrap_or(name).to_string())
        .collect();
    if let Some(helpers) = helpers.filter(|_| imported) {
        names.extend(helpers.names().iter().cloned());
        let renamed: Vec<String> = functions
            .iter()
            .filter(|(_, (_, original))| names.contains(original))
            .map(|(local, _)| local.clone())
            .collect();
        names.extend(renamed);
    }
    let mut names: Vec<String> = names.into_iter().collect();
    names.sort();
    // No pattern holds too many names: every scope is read.
    let pattern = mentions(&names);
    if pattern
        .as_ref()
        .is_some_and(|pattern| only_imported(text, pattern))
    {
        return Vec::new();
    }
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
    let lookup = |module: &str, name: &str| helpers.and_then(|helpers| helpers.get(module, name));
    let method_of = |class: &str, name: &str| match class.rsplit_once('.') {
        Some((prefix, class)) => lookup(&module_of(prefix, &modules), &format!("{class}.{name}")),
        None => own.get(&format!("{class}.{name}")).or_else(|| {
            let (module, original) = functions.get(class)?;
            lookup(module, &format!("{original}.{name}"))
        }),
    };
    let mut wanted: Vec<Option<bool>> = vec![None; scan.scopes.len()];
    let composed = text.contains("psycopg");
    let mut psycopg: Option<compose::Scan<'a>> = None;
    let resolve = |from: &Source, dotted: &str| {
        helpers.and_then(|helpers| helpers.fragment(from, dotted, module))
    };
    let mut calls = Vec::new();
    for (call, stack) in &scan.calls {
        if let Some(&scope) = stack.last() {
            let mentioned = *wanted[scope].get_or_insert_with(|| {
                let (start, end) = region(scan.scopes[scope], &lines, text.len());
                pattern
                    .as_ref()
                    .is_none_or(|pattern| pattern.is_match(&text[start..end]))
            });
            if !mentioned {
                continue;
            }
        }
        let helper = match call.func.as_ref() {
            Expr::Name(name) => own.get(name.id.as_str()).or_else(|| {
                let (module, original) = functions.get(name.id.as_str())?;
                lookup(module, original)
            }),
            Expr::Attribute(attr) => {
                let prefix = dotted(&attr.value).unwrap_or_default();
                let name = attr.attr.as_str();
                if let Some(found) = lookup(&module_of(&prefix, &modules), name) {
                    Some(found)
                } else if let Some(found) = method_of(&prefix, name) {
                    (found.kind != Kind::Instance).then_some(found)
                } else {
                    receiver_class(&attr.value, stack, &ctx)
                        .and_then(|class| method_of(&class, name))
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
        if literal.is_none() && forwarded(query, stack, &scan, &annotated, &marker) {
            continue;
        }
        let is_literal = literal.is_some();
        let mut built = Built {
            dynamic: literal.is_none(),
            query: literal,
            ..Built::default()
        };
        if built.query.is_none() {
            let rendered = if composed {
                let scan = psycopg.get_or_insert_with(|| compose::Scan::new(source));
                compose::render(scan, query, Some(&resolve)).unwrap_or(Rendered::Unchecked)
            } else {
                Rendered::None
            };
            match rendered {
                Rendered::Text(text) => (built.query, built.dynamic) = (Some(text), false),
                Rendered::Unchecked => built.dynamic = false,
                Rendered::None => {}
            }
            let around = || -> Vec<Node<'a>> { stack.iter().map(|s| scan.scopes[*s]).collect() };
            if let (Some(limit), None) = (variants, &built.query) {
                match query_variants(
                    source,
                    &around(),
                    call.range(),
                    query,
                    limit,
                    Some(&resolve),
                ) {
                    None => {}
                    Some(Found::Unchecked { composed, reason }) => {
                        built.reason = reason;
                        built.dynamic = built.dynamic && !composed;
                    }
                    Some(Found::Variants(found)) => {
                        built.query = found.first().map(|variant| variant.query.clone());
                        built.declared = found.first().and_then(|v| v.declared.clone());
                        built.dynamic = false;
                        built.variants = found;
                    }
                }
            } else if built.dynamic && matches!(query, Expr::Name(_) | Expr::Attribute(_)) {
                // Without query variants, a name still stands for the query of one `SQL[R](...)`.
                let found =
                    query_variants(source, &around(), call.range(), query, 1, Some(&resolve));
                if let Some(Found::Variants(found)) = found {
                    if let Some(declared) = found.into_iter().next().and_then(|v| v.declared) {
                        built.query = Some(declared.query.clone());
                        built.declared = Some(declared);
                        built.dynamic = false;
                    }
                }
            }
        }
        // `HelperCall._on`: an unmarked query is read against the database of its helper.
        if let Some(database) = &helper.database {
            let routed = |query: &mut String| {
                if !is_marked(query) {
                    query.insert_str(0, &format!("-- sqlty: db={database}\n"));
                }
            };
            let queries = built
                .variants
                .iter_mut()
                .flat_map(|variant| {
                    let declared = variant
                        .declared
                        .as_mut()
                        .map(|declared| &mut declared.query);
                    std::iter::once(&mut variant.query).chain(declared)
                })
                .chain(&mut built.query)
                .chain(built.declared.as_mut().map(|declared| &mut declared.query));
            queries.for_each(routed);
        }
        calls.push(Call {
            line: lines.line(call.start().to_usize()),
            dynamic: built.dynamic,
            query: built.query,
            reason: built.reason,
            variants: built.variants,
            declared: built.declared,
            row: argument(helper, call, helper.row.as_deref()),
            params: call_params(helper, call, stack, &ctx),
            end_line: Some(lines.line(call.end().to_usize().saturating_sub(1))),
            driver: None,
            single: None,
            configured: helper.configured,
            query_span: Some(query_span(call, query)),
            literal: is_literal,
        });
    }
    calls.sort_by_key(|call| (call.line, call.query_span));
    calls
}

/// Whether `text` names the helpers only on the lines that import them, so calls none.
fn only_imported(text: &str, pattern: &Regex) -> bool {
    // `from run(...)` can end a `yield` or a `raise` written over two lines.
    static IMPORT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[ \t\x0c]*(?:from[ \t]+[\w.]+[ \t]+import|import)[ \t][^;]*$")
            .expect("a valid pattern")
    });
    pattern.find_iter(text).all(|found| {
        let start = text[..found.start()]
            .rfind(['\n', '\r'])
            .map_or(0, |at| at + 1);
        let line = text[start..].split(['\n', '\r']).next().unwrap_or("");
        IMPORT.is_match(line)
    })
}

/// What a call's query argument is read as.
#[derive(Default)]
struct Built<'a> {
    query: Option<String>,
    dynamic: bool,
    reason: Option<String>,
    variants: Vec<Variant<'a>>,
    declared: Option<Declared<'a>>,
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

/// Where the query argument is, as `ast` places it: a bare generator spans the parentheses
/// of its call.
pub fn query_span(call: &ExprCall, query: &Expr) -> (usize, usize) {
    let range = match query {
        Expr::Generator(generator) if !generator.parenthesized => call.arguments.range(),
        _ => query.range(),
    };
    (range.start().to_usize(), range.end().to_usize())
}
