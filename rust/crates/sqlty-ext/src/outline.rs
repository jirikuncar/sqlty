//! A module as Python's `ast` nodes, built from the ruff tree, cut to what type extraction reads.
//!
//! Function bodies keep only the statements that hold a class definition, at any depth;
//! everything else is kept whole. Nodes carry the positions `ast.parse` gives them, so
//! `ast.dump(..., include_attributes=True)` of the outline equals that of `ast.parse` cut
//! the same way (`sqlty._source._outline`), except where `plans/2026-11-07-python-ast-in-rust.md`
//! lists a difference.

use std::borrow::Cow;

use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBytes, PyComplex, PyDict, PyList};
use pyo3::{intern, IntoPyObjectExt};
use ruff_python_ast::token::{TokenKind, Tokens};
use ruff_python_ast::{
    self as ruff, BoolOp, CmpOp, ConversionFlag, ExceptHandler, Expr, ExprContext,
    InterpolatedStringElement, InterpolatedStringElements, Number, Operator, Pattern, Singleton,
    Stmt, TypeParam, UnaryOp,
};
use ruff_text_size::{Ranged, TextRange, TextSize};
use sqlty_python::scopes::Lines;
use sqlty_python::unparse;

macro_rules! ast_types {
    (classes: $($class:ident),*; instances: $($instance:ident),*) => {
        #[allow(non_snake_case)]
        struct Ast {
            $($class: Py<PyAny>,)*
            $($instance: Py<PyAny>,)*
            /// Whether imports have `is_lazy`, new in Python 3.15.
            lazy: bool,
        }

        impl Ast {
            fn new(py: Python<'_>) -> PyResult<Self> {
                let module = py.import("ast")?;
                Ok(Self {
                    $($class: module.getattr(stringify!($class))?.unbind(),)*
                    $($instance: module.getattr(stringify!($instance))?.call0()?.unbind(),)*
                    lazy: module.getattr("Import")?.getattr("_fields")?.contains("is_lazy")?,
                })
            }
        }
    };
}

ast_types! {
    classes: Module, FunctionDef, AsyncFunctionDef, ClassDef, Return, Delete, Assign, TypeAlias,
        AugAssign, AnnAssign, For, AsyncFor, While, If, With, AsyncWith, Match, Raise, Try,
        TryStar, Assert, Import, ImportFrom, Global, Nonlocal, Expr, Pass, Break, Continue,
        BoolOp, NamedExpr, BinOp, UnaryOp, Lambda, IfExp, Dict, Set, ListComp, SetComp, DictComp,
        GeneratorExp, Await, Yield, YieldFrom, Compare, Call, FormattedValue, Interpolation,
        JoinedStr, TemplateStr, Constant, Attribute, Subscript, Starred, Name, List, Tuple, Slice,
        comprehension, ExceptHandler, arguments, arg, keyword, alias, withitem, match_case,
        MatchValue, MatchSingleton, MatchSequence, MatchMapping, MatchClass, MatchStar, MatchAs,
        MatchOr, TypeVar, ParamSpec, TypeVarTuple;
    instances: Load, Store, Del, And, Or, Add, Sub, Mult, MatMult, Div, Mod, Pow, LShift, RShift,
        BitOr, BitXor, BitAnd, FloorDiv, Invert, Not, UAdd, USub, Eq, NotEq, Lt, LtE, Gt, GtE, Is,
        IsNot, In, NotIn
}

static AST: PyOnceLock<Ast> = PyOnceLock::new();

type Node<'py> = Bound<'py, PyAny>;
/// A module and its classes.
pub type Outline<'py> = (Node<'py>, Vec<Node<'py>>);

/// The outline of `source` and its classes in `ParsedSource.classes` order, or `None` where
/// `sqlty_python::parse::module` does not read it.
pub fn outline<'py>(py: Python<'py>, source: &str) -> PyResult<Option<Outline<'py>>> {
    // `ast.parse` reads `\r\n` and `\r` as `\n`, in string values too.
    let text: Cow<'_, str> = if source.contains('\r') {
        Cow::Owned(source.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(source)
    };
    let Some(parsed) = crate::detach(py, || sqlty_python::parse::module(&text)) else {
        return Ok(None);
    };
    let ast = AST.get_or_try_init(py, || Ast::new(py))?;
    if !ast.lazy && !crate::detach(py, || sqlty_python::parse::reads_on_314(&text)) {
        return Ok(None);
    }
    let mut builder = Builder {
        py,
        ast,
        source: &text,
        lines: Lines::new(&text),
        tokens: parsed.tokens(),
        classes: Vec::new(),
    };
    let body = builder.body(&parsed.syntax().body, 1, false)?;
    let module = builder
        .ast
        .Module
        .bind(py)
        .call1((body, PyList::empty(py)))?;
    let mut classes = builder.classes;
    classes.sort_by_key(|(depth, _)| *depth);
    Ok(Some((
        module,
        classes.into_iter().map(|(_, node)| node).collect(),
    )))
}

struct Builder<'py, 'a> {
    py: Python<'py>,
    ast: &'a Ast,
    source: &'a str,
    lines: Lines,
    tokens: &'a Tokens,
    classes: Vec<(usize, Node<'py>)>,
}

/// Whether `stmt` is or holds a class definition.
fn holds_class(stmt: &Stmt) -> bool {
    let any = |body: &[Stmt]| body.iter().any(holds_class);
    match stmt {
        Stmt::ClassDef(_) => true,
        Stmt::FunctionDef(node) => any(&node.body),
        Stmt::For(node) => any(&node.body) || any(&node.orelse),
        Stmt::While(node) => any(&node.body) || any(&node.orelse),
        Stmt::If(node) => any(&node.body) || node.elif_else_clauses.iter().any(|c| any(&c.body)),
        Stmt::With(node) => any(&node.body),
        Stmt::Match(node) => node.cases.iter().any(|c| any(&c.body)),
        Stmt::Try(node) => {
            any(&node.body)
                || any(&node.orelse)
                || any(&node.finalbody)
                || node
                    .handlers
                    .iter()
                    .any(|ExceptHandler::ExceptHandler(h)| any(&h.body))
        }
        _ => false,
    }
}

/// A run of literal text in an f- or t-string, which `ast` merges with its neighbours.
struct Piece<'s> {
    text: Cow<'s, str>,
    range: TextRange,
    unicode: bool,
}

impl<'py> Builder<'py, '_> {
    fn position(&self, offset: TextSize) -> (usize, usize) {
        let offset = offset.to_usize();
        let line = self.lines.line(offset);
        (line, offset - self.lines.start(line, self.source.len()))
    }

    fn node(
        &self,
        class: &Py<PyAny>,
        fields: impl pyo3::call::PyCallArgs<'py>,
        range: TextRange,
    ) -> PyResult<Node<'py>> {
        let py = self.py;
        let (line, col) = self.position(range.start());
        let (end_line, end_col) = self.position(range.end());
        let kwargs = PyDict::new(py);
        kwargs.set_item(intern!(py, "lineno"), line)?;
        kwargs.set_item(intern!(py, "col_offset"), col)?;
        kwargs.set_item(intern!(py, "end_lineno"), end_line)?;
        kwargs.set_item(intern!(py, "end_col_offset"), end_col)?;
        class.bind(py).call(fields, Some(&kwargs))
    }

    fn bare(
        &self,
        class: &Py<PyAny>,
        fields: impl pyo3::call::PyCallArgs<'py>,
    ) -> PyResult<Node<'py>> {
        class.bind(self.py).call1(fields)
    }

    fn instance(&self, node: &Py<PyAny>) -> Node<'py> {
        node.bind(self.py).clone()
    }

    /// The `def`, `async` or `class` keyword after the decorators, where `ast` starts the node.
    fn keyword(&self, from: TextSize) -> TextSize {
        let tokens: &[ruff_python_ast::token::Token] = self.tokens;
        let first = tokens.partition_point(|token| token.end() <= from);
        tokens[first..]
            .iter()
            .find(|t| {
                matches!(
                    t.kind(),
                    TokenKind::Def | TokenKind::Async | TokenKind::Class
                )
            })
            .map_or(from, Ranged::start)
    }

    fn body(&mut self, body: &[Stmt], depth: usize, cut: bool) -> PyResult<Vec<Node<'py>>> {
        body.iter()
            .filter(|stmt| !cut || holds_class(stmt))
            .map(|stmt| self.stmt(stmt, depth, cut))
            .collect()
    }

    fn exprs(&mut self, exprs: &[Expr]) -> PyResult<Vec<Node<'py>>> {
        exprs.iter().map(|expr| self.expr(expr)).collect()
    }

    fn opt(&mut self, expr: Option<&Expr>) -> PyResult<Option<Node<'py>>> {
        expr.map(|expr| self.expr(expr)).transpose()
    }

    fn defined_range(&self, range: TextRange, decorators: &[ruff::Decorator]) -> TextRange {
        match decorators.last() {
            Some(last) => TextRange::new(self.keyword(last.end()), range.end()),
            None => range,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn stmt(&mut self, stmt: &Stmt, depth: usize, cut: bool) -> PyResult<Node<'py>> {
        let ast = self.ast;
        let range = stmt.range();
        match stmt {
            Stmt::FunctionDef(node) => {
                let decorators: Vec<Node<'py>> = node
                    .decorator_list
                    .iter()
                    .map(|d| self.expr(&d.expression))
                    .collect::<PyResult<_>>()?;
                let fields = (
                    node.name.as_str(),
                    self.parameters(Some(&node.parameters))?,
                    self.body(&node.body, depth + 1, true)?,
                    decorators,
                    self.opt(node.returns.as_deref())?,
                    None::<&str>,
                    self.type_params(node.type_params.as_deref())?,
                );
                let class = if node.is_async {
                    &ast.AsyncFunctionDef
                } else {
                    &ast.FunctionDef
                };
                self.node(
                    class,
                    fields,
                    self.defined_range(range, &node.decorator_list),
                )
            }
            Stmt::ClassDef(node) => {
                let decorators: Vec<Node<'py>> = node
                    .decorator_list
                    .iter()
                    .map(|d| self.expr(&d.expression))
                    .collect::<PyResult<_>>()?;
                let (bases, keywords) = match node.arguments.as_deref() {
                    Some(arguments) => (
                        self.exprs(&arguments.args)?,
                        self.keywords(&arguments.keywords)?,
                    ),
                    None => (Vec::new(), Vec::new()),
                };
                let fields = (
                    node.name.as_str(),
                    bases,
                    keywords,
                    self.body(&node.body, depth + 1, false)?,
                    decorators,
                    self.type_params(node.type_params.as_deref())?,
                );
                let built = self.node(
                    &ast.ClassDef,
                    fields,
                    self.defined_range(range, &node.decorator_list),
                )?;
                self.classes.push((depth, built.clone()));
                Ok(built)
            }
            Stmt::Return(node) => {
                let value = self.opt(node.value.as_deref())?;
                self.node(&ast.Return, (value,), range)
            }
            Stmt::Delete(node) => {
                let targets = self.exprs(&node.targets)?;
                self.node(&ast.Delete, (targets,), range)
            }
            Stmt::TypeAlias(node) => {
                let fields = (
                    self.expr(&node.name)?,
                    self.type_params(node.type_params.as_deref())?,
                    self.expr(&node.value)?,
                );
                self.node(&ast.TypeAlias, fields, range)
            }
            Stmt::Assign(node) => {
                let fields = (
                    self.exprs(&node.targets)?,
                    self.expr(&node.value)?,
                    None::<&str>,
                );
                self.node(&ast.Assign, fields, range)
            }
            Stmt::AugAssign(node) => {
                let fields = (
                    self.expr(&node.target)?,
                    self.operator(node.op),
                    self.expr(&node.value)?,
                );
                self.node(&ast.AugAssign, fields, range)
            }
            Stmt::AnnAssign(node) => {
                let fields = (
                    self.expr(&node.target)?,
                    self.expr(&node.annotation)?,
                    self.opt(node.value.as_deref())?,
                    i32::from(node.simple),
                );
                self.node(&ast.AnnAssign, fields, range)
            }
            Stmt::For(node) => {
                let fields = (
                    self.expr(&node.target)?,
                    self.expr(&node.iter)?,
                    self.body(&node.body, depth + 1, cut)?,
                    self.body(&node.orelse, depth + 1, cut)?,
                    None::<&str>,
                );
                let class = if node.is_async {
                    &ast.AsyncFor
                } else {
                    &ast.For
                };
                self.node(class, fields, range)
            }
            Stmt::While(node) => {
                let fields = (
                    self.expr(&node.test)?,
                    self.body(&node.body, depth + 1, cut)?,
                    self.body(&node.orelse, depth + 1, cut)?,
                );
                self.node(&ast.While, fields, range)
            }
            Stmt::If(node) => self.branch(
                &node.test,
                &node.body,
                &node.elif_else_clauses,
                range,
                depth,
                cut,
            ),
            Stmt::With(node) => {
                let items: Vec<Node<'py>> = node
                    .items
                    .iter()
                    .map(|item| {
                        let fields = (
                            self.expr(&item.context_expr)?,
                            self.opt(item.optional_vars.as_deref())?,
                        );
                        self.bare(&ast.withitem, fields)
                    })
                    .collect::<PyResult<_>>()?;
                let fields = (items, self.body(&node.body, depth + 1, cut)?, None::<&str>);
                let class = if node.is_async {
                    &ast.AsyncWith
                } else {
                    &ast.With
                };
                self.node(class, fields, range)
            }
            Stmt::Match(node) => {
                let subject = self.expr(&node.subject)?;
                let cases: Vec<Node<'py>> = node
                    .cases
                    .iter()
                    .filter(|case| !cut || case.body.iter().any(holds_class))
                    .map(|case| {
                        let fields = (
                            self.pattern(&case.pattern)?,
                            self.opt(case.guard.as_deref())?,
                            self.body(&case.body, depth + 2, cut)?,
                        );
                        self.bare(&ast.match_case, fields)
                    })
                    .collect::<PyResult<_>>()?;
                self.node(&ast.Match, (subject, cases), range)
            }
            Stmt::Raise(node) => {
                let fields = (
                    self.opt(node.exc.as_deref())?,
                    self.opt(node.cause.as_deref())?,
                );
                self.node(&ast.Raise, fields, range)
            }
            Stmt::Try(node) => {
                let handlers: Vec<Node<'py>> = node
                    .handlers
                    .iter()
                    .filter(|ExceptHandler::ExceptHandler(h)| {
                        !cut || h.body.iter().any(holds_class)
                    })
                    .map(|ExceptHandler::ExceptHandler(h)| {
                        let fields = (
                            self.opt(h.type_.as_deref())?,
                            h.name.as_ref().map(ruff::Identifier::as_str),
                            self.body(&h.body, depth + 2, cut)?,
                        );
                        self.node(&ast.ExceptHandler, fields, h.range())
                    })
                    .collect::<PyResult<_>>()?;
                let fields = (
                    self.body(&node.body, depth + 1, cut)?,
                    handlers,
                    self.body(&node.orelse, depth + 1, cut)?,
                    self.body(&node.finalbody, depth + 1, cut)?,
                );
                let class = if node.is_star { &ast.TryStar } else { &ast.Try };
                self.node(class, fields, range)
            }
            Stmt::Assert(node) => {
                let fields = (self.expr(&node.test)?, self.opt(node.msg.as_deref())?);
                self.node(&ast.Assert, fields, range)
            }
            Stmt::Import(node) => {
                let names = self.aliases(&node.names)?;
                if ast.lazy {
                    self.node(&ast.Import, (names, i32::from(node.is_lazy)), range)
                } else {
                    self.node(&ast.Import, (names,), range)
                }
            }
            Stmt::ImportFrom(node) => {
                let module = node.module.as_ref().map(ruff::Identifier::as_str);
                let names = self.aliases(&node.names)?;
                if ast.lazy {
                    let fields = (module, names, node.level, i32::from(node.is_lazy));
                    self.node(&ast.ImportFrom, fields, range)
                } else {
                    self.node(&ast.ImportFrom, (module, names, node.level), range)
                }
            }
            Stmt::Global(node) => {
                let names: Vec<&str> = node.names.iter().map(ruff::Identifier::as_str).collect();
                self.node(&ast.Global, (names,), range)
            }
            Stmt::Nonlocal(node) => {
                let names: Vec<&str> = node.names.iter().map(ruff::Identifier::as_str).collect();
                self.node(&ast.Nonlocal, (names,), range)
            }
            Stmt::Expr(node) => {
                let value = self.expr(&node.value)?;
                self.node(&ast.Expr, (value,), range)
            }
            Stmt::Pass(_) => self.node(&ast.Pass, (), range),
            Stmt::Break(_) => self.node(&ast.Break, (), range),
            Stmt::Continue(_) => self.node(&ast.Continue, (), range),
            Stmt::IpyEscapeCommand(_) => Err(pyo3::exceptions::PySyntaxError::new_err(
                "IPython escape command",
            )),
        }
    }

    /// `if`, each `elif` nested in the `orelse` of the one before it as `ast` does.
    fn branch(
        &mut self,
        test: &Expr,
        body: &[Stmt],
        clauses: &[ruff::ElifElseClause],
        range: TextRange,
        depth: usize,
        cut: bool,
    ) -> PyResult<Node<'py>> {
        let test = self.expr(test)?;
        let body = self.body(body, depth + 1, cut)?;
        let orelse = match clauses {
            [] => Vec::new(),
            [clause, rest @ ..] => match &clause.test {
                None => self.body(&clause.body, depth + 1, cut)?,
                Some(elif) => {
                    let holds = clause.body.iter().any(holds_class)
                        || rest.iter().any(|c| c.body.iter().any(holds_class));
                    if cut && !holds {
                        Vec::new()
                    } else {
                        let nested = TextRange::new(clause.start(), range.end());
                        vec![self.branch(elif, &clause.body, rest, nested, depth + 1, cut)?]
                    }
                }
            },
        };
        self.node(&self.ast.If, (test, body, orelse), range)
    }

    fn aliases(&self, names: &[ruff::Alias]) -> PyResult<Vec<Node<'py>>> {
        names
            .iter()
            .map(|alias| {
                let fields = (
                    alias.name.as_str(),
                    alias.asname.as_ref().map(ruff::Identifier::as_str),
                );
                self.node(&self.ast.alias, fields, alias.range())
            })
            .collect()
    }

    fn keywords(&mut self, keywords: &[ruff::Keyword]) -> PyResult<Vec<Node<'py>>> {
        keywords
            .iter()
            .map(|keyword| {
                let fields = (
                    keyword.arg.as_ref().map(ruff::Identifier::as_str),
                    self.expr(&keyword.value)?,
                );
                self.node(&self.ast.keyword, fields, keyword.range())
            })
            .collect()
    }

    fn parameter(&mut self, parameter: &ruff::Parameter) -> PyResult<Node<'py>> {
        let annotation = self.opt(parameter.annotation.as_deref())?;
        // A parenthesized annotation ends at its `)`, which `parameter` includes.
        let end = match parameter.annotation {
            Some(_) => parameter.end(),
            None => parameter.name.end(),
        };
        let range = TextRange::new(parameter.name.start(), end);
        self.node(
            &self.ast.arg,
            (parameter.name.as_str(), annotation, None::<&str>),
            range,
        )
    }

    fn parameters(&mut self, parameters: Option<&ruff::Parameters>) -> PyResult<Node<'py>> {
        let empty = ruff::Parameters::default();
        let parameters = parameters.unwrap_or(&empty);
        let mut defaults = Vec::new();
        let mut positional = |builder: &mut Self, list: &[ruff::ParameterWithDefault]| {
            list.iter()
                .map(|p| {
                    if let Some(default) = &p.default {
                        defaults.push(builder.expr(default)?);
                    }
                    builder.parameter(&p.parameter)
                })
                .collect::<PyResult<Vec<_>>>()
        };
        let posonly = positional(self, &parameters.posonlyargs)?;
        let args = positional(self, &parameters.args)?;
        let vararg = parameters
            .vararg
            .as_deref()
            .map(|p| self.parameter(p))
            .transpose()?;
        let mut kw_defaults = Vec::new();
        let kwonly = parameters
            .kwonlyargs
            .iter()
            .map(|p| {
                kw_defaults.push(self.opt(p.default.as_deref())?);
                self.parameter(&p.parameter)
            })
            .collect::<PyResult<Vec<_>>>()?;
        let kwarg = parameters
            .kwarg
            .as_deref()
            .map(|p| self.parameter(p))
            .transpose()?;
        let fields = (posonly, args, vararg, kwonly, kw_defaults, kwarg, defaults);
        self.bare(&self.ast.arguments, fields)
    }

    fn type_params(&mut self, params: Option<&ruff::TypeParams>) -> PyResult<Vec<Node<'py>>> {
        let Some(params) = params else {
            return Ok(Vec::new());
        };
        params
            .type_params
            .iter()
            .map(|param| match param {
                TypeParam::TypeVar(p) => {
                    let fields = (
                        p.name.as_str(),
                        self.opt(p.bound.as_deref())?,
                        self.opt(p.default.as_deref())?,
                    );
                    self.node(&self.ast.TypeVar, fields, p.range())
                }
                TypeParam::ParamSpec(p) => {
                    let fields = (p.name.as_str(), self.opt(p.default.as_deref())?);
                    self.node(&self.ast.ParamSpec, fields, p.range())
                }
                TypeParam::TypeVarTuple(p) => {
                    let fields = (p.name.as_str(), self.opt(p.default.as_deref())?);
                    self.node(&self.ast.TypeVarTuple, fields, p.range())
                }
            })
            .collect()
    }

    fn pattern(&mut self, pattern: &Pattern) -> PyResult<Node<'py>> {
        let ast = self.ast;
        let range = pattern.range();
        let patterns = |builder: &mut Self, list: &[Pattern]| {
            list.iter()
                .map(|p| builder.pattern(p))
                .collect::<PyResult<Vec<_>>>()
        };
        match pattern {
            Pattern::MatchValue(p) => {
                let value = self.expr(&p.value)?;
                self.node(&ast.MatchValue, (value,), range)
            }
            Pattern::MatchSingleton(p) => {
                let value = match p.value {
                    Singleton::None => self.py.None().into_bound(self.py),
                    Singleton::True => true.into_bound_py_any(self.py)?,
                    Singleton::False => false.into_bound_py_any(self.py)?,
                };
                self.node(&ast.MatchSingleton, (value,), range)
            }
            Pattern::MatchSequence(p) => {
                let list = patterns(self, &p.patterns)?;
                self.node(&ast.MatchSequence, (list,), range)
            }
            Pattern::MatchMapping(p) => {
                let fields = (
                    self.exprs(&p.keys)?,
                    patterns(self, &p.patterns)?,
                    p.rest.as_ref().map(ruff::Identifier::as_str),
                );
                self.node(&ast.MatchMapping, fields, range)
            }
            Pattern::MatchClass(p) => {
                let attrs: Vec<&str> = p
                    .arguments
                    .keywords
                    .iter()
                    .map(|k| k.attr.as_str())
                    .collect();
                let kwd_patterns = p
                    .arguments
                    .keywords
                    .iter()
                    .map(|k| self.pattern(&k.pattern))
                    .collect::<PyResult<Vec<_>>>()?;
                let fields = (
                    self.expr(&p.cls)?,
                    patterns(self, &p.arguments.patterns)?,
                    attrs,
                    kwd_patterns,
                );
                self.node(&ast.MatchClass, fields, range)
            }
            Pattern::MatchStar(p) => {
                let name = p.name.as_ref().map(ruff::Identifier::as_str);
                self.node(&ast.MatchStar, (name,), range)
            }
            Pattern::MatchAs(p) => {
                let fields = (
                    p.pattern.as_deref().map(|p| self.pattern(p)).transpose()?,
                    p.name.as_ref().map(ruff::Identifier::as_str),
                );
                self.node(&ast.MatchAs, fields, range)
            }
            Pattern::MatchOr(p) => {
                let list = patterns(self, &p.patterns)?;
                self.node(&ast.MatchOr, (list,), range)
            }
        }
    }

    fn context(&self, ctx: ExprContext) -> Node<'py> {
        self.instance(match ctx {
            ExprContext::Store => &self.ast.Store,
            ExprContext::Del => &self.ast.Del,
            ExprContext::Load | ExprContext::Invalid => &self.ast.Load,
        })
    }

    fn operator(&self, op: Operator) -> Node<'py> {
        let ast = self.ast;
        self.instance(match op {
            Operator::Add => &ast.Add,
            Operator::Sub => &ast.Sub,
            Operator::Mult => &ast.Mult,
            Operator::MatMult => &ast.MatMult,
            Operator::Div => &ast.Div,
            Operator::Mod => &ast.Mod,
            Operator::Pow => &ast.Pow,
            Operator::LShift => &ast.LShift,
            Operator::RShift => &ast.RShift,
            Operator::BitOr => &ast.BitOr,
            Operator::BitXor => &ast.BitXor,
            Operator::BitAnd => &ast.BitAnd,
            Operator::FloorDiv => &ast.FloorDiv,
        })
    }

    fn comparison(&self, op: CmpOp) -> Node<'py> {
        let ast = self.ast;
        self.instance(match op {
            CmpOp::Eq => &ast.Eq,
            CmpOp::NotEq => &ast.NotEq,
            CmpOp::Lt => &ast.Lt,
            CmpOp::LtE => &ast.LtE,
            CmpOp::Gt => &ast.Gt,
            CmpOp::GtE => &ast.GtE,
            CmpOp::Is => &ast.Is,
            CmpOp::IsNot => &ast.IsNot,
            CmpOp::In => &ast.In,
            CmpOp::NotIn => &ast.NotIn,
        })
    }

    fn comprehensions(&mut self, generators: &[ruff::Comprehension]) -> PyResult<Vec<Node<'py>>> {
        generators
            .iter()
            .map(|c| {
                let fields = (
                    self.expr(&c.target)?,
                    self.expr(&c.iter)?,
                    self.exprs(&c.ifs)?,
                    i32::from(c.is_async),
                );
                self.bare(&self.ast.comprehension, fields)
            })
            .collect()
    }

    fn constant(
        &self,
        value: impl IntoPyObject<'py>,
        kind: Option<&str>,
        range: TextRange,
    ) -> PyResult<Node<'py>> {
        let value = value.into_bound_py_any(self.py)?;
        self.node(&self.ast.Constant, (value, kind), range)
    }

    #[recursive::recursive]
    #[allow(clippy::too_many_lines)]
    fn expr(&mut self, expr: &Expr) -> PyResult<Node<'py>> {
        let ast = self.ast;
        let py = self.py;
        let range = expr.range();
        match expr {
            Expr::BoolOp(e) => {
                let op = self.instance(match e.op {
                    BoolOp::And => &ast.And,
                    BoolOp::Or => &ast.Or,
                });
                let values = self.exprs(&e.values)?;
                self.node(&ast.BoolOp, (op, values), range)
            }
            Expr::Named(e) => {
                let fields = (self.expr(&e.target)?, self.expr(&e.value)?);
                self.node(&ast.NamedExpr, fields, range)
            }
            Expr::BinOp(e) => {
                let fields = (
                    self.expr(&e.left)?,
                    self.operator(e.op),
                    self.expr(&e.right)?,
                );
                self.node(&ast.BinOp, fields, range)
            }
            Expr::UnaryOp(e) => {
                let op = self.instance(match e.op {
                    UnaryOp::Invert => &ast.Invert,
                    UnaryOp::Not => &ast.Not,
                    UnaryOp::UAdd => &ast.UAdd,
                    UnaryOp::USub => &ast.USub,
                });
                let operand = self.expr(&e.operand)?;
                self.node(&ast.UnaryOp, (op, operand), range)
            }
            Expr::Lambda(e) => {
                let fields = (
                    self.parameters(e.parameters.as_deref())?,
                    self.expr(&e.body)?,
                );
                self.node(&ast.Lambda, fields, range)
            }
            Expr::If(e) => {
                let fields = (
                    self.expr(&e.test)?,
                    self.expr(&e.body)?,
                    self.expr(&e.orelse)?,
                );
                self.node(&ast.IfExp, fields, range)
            }
            Expr::Dict(e) => {
                let mut keys = Vec::with_capacity(e.items.len());
                let mut values = Vec::with_capacity(e.items.len());
                for item in &e.items {
                    keys.push(self.opt(item.key.as_ref())?);
                    values.push(self.expr(&item.value)?);
                }
                self.node(&ast.Dict, (keys, values), range)
            }
            Expr::Set(e) => {
                let elts = self.exprs(&e.elts)?;
                self.node(&ast.Set, (elts,), range)
            }
            Expr::ListComp(e) => {
                let fields = (self.expr(&e.elt)?, self.comprehensions(&e.generators)?);
                self.node(&ast.ListComp, fields, range)
            }
            Expr::SetComp(e) => {
                let fields = (self.expr(&e.elt)?, self.comprehensions(&e.generators)?);
                self.node(&ast.SetComp, fields, range)
            }
            Expr::DictComp(e) => {
                // `{**d for d in ds}` is `DictComp(key=d, value=None)` in `ast`.
                let (key, value) = match e.key.as_deref() {
                    Some(key) => (self.expr(key)?, Some(self.expr(&e.value)?)),
                    None => (self.expr(&e.value)?, None),
                };
                let fields = (key, value, self.comprehensions(&e.generators)?);
                self.node(&ast.DictComp, fields, range)
            }
            Expr::Generator(e) => {
                let fields = (self.expr(&e.elt)?, self.comprehensions(&e.generators)?);
                self.node(&ast.GeneratorExp, fields, range)
            }
            Expr::Await(e) => {
                let value = self.expr(&e.value)?;
                self.node(&ast.Await, (value,), range)
            }
            Expr::Yield(e) => {
                let value = self.opt(e.value.as_deref())?;
                self.node(&ast.Yield, (value,), range)
            }
            Expr::YieldFrom(e) => {
                let value = self.expr(&e.value)?;
                self.node(&ast.YieldFrom, (value,), range)
            }
            Expr::Compare(e) => {
                let ops: Vec<Node<'py>> = e.ops.iter().map(|op| self.comparison(*op)).collect();
                let (left, comparators) =
                    e.operands.split_first().expect("a comparison has operands");
                let fields = (self.expr(left)?, ops, self.exprs(comparators)?);
                self.node(&ast.Compare, fields, range)
            }
            Expr::Call(e) => {
                let func = self.expr(&e.func)?;
                // `ast` puts the parentheses of the call around a lone generator argument.
                let args = match (&*e.arguments.args, &*e.arguments.keywords) {
                    ([Expr::Generator(g)], []) if !g.parenthesized => {
                        let fields = (self.expr(&g.elt)?, self.comprehensions(&g.generators)?);
                        vec![self.node(&ast.GeneratorExp, fields, e.arguments.range())?]
                    }
                    _ => self.exprs(&e.arguments.args)?,
                };
                let keywords = self.keywords(&e.arguments.keywords)?;
                self.node(&ast.Call, (func, args, keywords), range)
            }
            Expr::FString(e) => {
                let mut pieces = Vec::new();
                let mut values = Vec::new();
                for part in &e.value {
                    match part {
                        ruff::FStringPartRef::Literal(literal) => pieces.push(Piece {
                            text: Cow::Borrowed(&literal.value),
                            range: literal.range(),
                            unicode: literal.flags.prefix().is_unicode(),
                        }),
                        ruff::FStringPartRef::FString(fstring) => {
                            self.elements(&fstring.elements, &mut pieces, &mut values, false)?;
                        }
                    }
                }
                self.flush(&mut pieces, &mut values)?;
                self.node(&ast.JoinedStr, (values,), range)
            }
            Expr::TString(e) => {
                let mut pieces = Vec::new();
                let mut values = Vec::new();
                for tstring in e.value.as_slice() {
                    self.elements(&tstring.elements, &mut pieces, &mut values, true)?;
                }
                self.flush(&mut pieces, &mut values)?;
                self.node(&ast.TemplateStr, (values,), range)
            }
            Expr::StringLiteral(e) => {
                let kind = e.value.is_unicode().then_some("u");
                self.constant(e.value.to_str(), kind, range)
            }
            Expr::BytesLiteral(e) => {
                let bytes: Vec<u8> = e.value.bytes().collect();
                self.constant(PyBytes::new(py, &bytes), None, range)
            }
            Expr::NumberLiteral(e) => match &e.value {
                Number::Int(int) => match int.as_u64() {
                    Some(small) => self.constant(small, None, range),
                    None => {
                        let text = &self.source[range];
                        let value = py.get_type::<pyo3::types::PyInt>().call1((text, 0))?;
                        self.constant(value, None, range)
                    }
                },
                Number::Float(value) => self.constant(*value, None, range),
                Number::Complex { real, imag } => {
                    self.constant(PyComplex::from_doubles(py, *real, *imag), None, range)
                }
            },
            Expr::BooleanLiteral(e) => self.constant(e.value, None, range),
            Expr::NoneLiteral(_) => self.constant(py.None(), None, range),
            Expr::EllipsisLiteral(_) => self.constant(py.Ellipsis(), None, range),
            Expr::Attribute(e) => {
                let fields = (self.expr(&e.value)?, e.attr.as_str(), self.context(e.ctx));
                self.node(&ast.Attribute, fields, range)
            }
            Expr::Subscript(e) => {
                let fields = (
                    self.expr(&e.value)?,
                    self.expr(&e.slice)?,
                    self.context(e.ctx),
                );
                self.node(&ast.Subscript, fields, range)
            }
            Expr::Starred(e) => {
                let fields = (self.expr(&e.value)?, self.context(e.ctx));
                self.node(&ast.Starred, fields, range)
            }
            Expr::Name(e) => self.node(&ast.Name, (e.id.as_str(), self.context(e.ctx)), range),
            Expr::List(e) => {
                let fields = (self.exprs(&e.elts)?, self.context(e.ctx));
                self.node(&ast.List, fields, range)
            }
            Expr::Tuple(e) => {
                let fields = (self.exprs(&e.elts)?, self.context(e.ctx));
                self.node(&ast.Tuple, fields, range)
            }
            Expr::Slice(e) => {
                let fields = (
                    self.opt(e.lower.as_deref())?,
                    self.opt(e.upper.as_deref())?,
                    self.opt(e.step.as_deref())?,
                );
                self.node(&ast.Slice, fields, range)
            }
            Expr::IpyEscapeCommand(_) => Err(pyo3::exceptions::PySyntaxError::new_err(
                "IPython escape command",
            )),
        }
    }

    /// The literal text and replacement fields of an f- or t-string, the text left in
    /// `pieces` to merge with what follows.
    fn elements<'e>(
        &mut self,
        elements: &'e InterpolatedStringElements,
        pieces: &mut Vec<Piece<'e>>,
        values: &mut Vec<Node<'py>>,
        template: bool,
    ) -> PyResult<()> {
        for element in elements {
            match element {
                InterpolatedStringElement::Literal(literal) => pieces.push(Piece {
                    text: Cow::Borrowed(&literal.value),
                    range: literal.range(),
                    unicode: false,
                }),
                InterpolatedStringElement::Interpolation(field) => {
                    let inner = field.start() + TextSize::from(1);
                    let mut conversion = field.conversion;
                    if field.debug_text.is_some() {
                        let text = unparse::field_text(field, self.source);
                        pieces.push(Piece {
                            range: TextRange::at(inner, TextSize::of(&text)),
                            text: Cow::Owned(text),
                            unicode: false,
                        });
                        if conversion == ConversionFlag::None && field.format_spec.is_none() {
                            conversion = ConversionFlag::Repr;
                        }
                    }
                    self.flush(pieces, values)?;
                    let value = self.expr(&field.expression)?;
                    let spec = field
                        .format_spec
                        .as_deref()
                        .map(|spec| {
                            let mut inner_pieces = Vec::new();
                            let mut inner_values = Vec::new();
                            self.elements(
                                &spec.elements,
                                &mut inner_pieces,
                                &mut inner_values,
                                false,
                            )?;
                            self.flush(&mut inner_pieces, &mut inner_values)?;
                            let range =
                                TextRange::new(spec.start() - TextSize::from(1), spec.end());
                            self.node(&self.ast.JoinedStr, (inner_values,), range)
                        })
                        .transpose()?;
                    let conversion = conversion as i8;
                    let node = if template {
                        let text = unparse::interpolation_text(field, self.source);
                        self.node(
                            &self.ast.Interpolation,
                            (value, text, conversion, spec),
                            field.range(),
                        )?
                    } else {
                        self.node(
                            &self.ast.FormattedValue,
                            (value, conversion, spec),
                            field.range(),
                        )?
                    };
                    values.push(node);
                }
            }
        }
        Ok(())
    }

    /// The merged text in `pieces` as one `Constant`, unless it is empty.
    fn flush(&self, pieces: &mut Vec<Piece<'_>>, values: &mut Vec<Node<'py>>) -> PyResult<()> {
        let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
            return Ok(());
        };
        let text: String = pieces.iter().map(|p| p.text.as_ref()).collect();
        let range = TextRange::new(first.range.start(), last.range.end());
        let kind = first.unicode.then_some("u");
        pieces.clear();
        if !text.is_empty() {
            values.push(self.constant(text, kind, range)?);
        }
        Ok(())
    }
}
