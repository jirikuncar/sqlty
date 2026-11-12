//! The calls `sqlty._helpers.find_helper_calls` resolves against the helper index, with what
//! it reads of their arguments, so a file whose helper calls all pass a string literal is
//! checked without Python's `ast`.
//!
//! As `ParsedSource.calls_to` does, a call in a function or class whose source (decorators
//! included) never matches one of `names` is left out; module-level calls are all kept.

use std::sync::{Arc, Mutex};

use regex::Regex;
use ruff_python_ast::{Expr, ExprCall};
use ruff_text_size::{Ranged, TextRange};

use crate::module::Imports;
use crate::scopes::{dotted, Context, Lines, Node, Param, Scan, Scope};

/// An argument of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    /// `*value`.
    pub starred: bool,
    /// Byte offsets.
    pub span: (usize, usize),
    /// The text of a string literal.
    pub text: Option<String>,
    /// Its type, as `sqlty._params.arg_type` reads it.
    pub kind: Option<String>,
    /// What it passes as a tuple, list or dict literal, as `sqlty._params.literal_params` reads it.
    pub params: Option<Vec<Param>>,
}

/// A call of a name, `f(...)`, or of an attribute of a dotted name, `a.b.f(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The lines the call starts and ends on.
    pub lines: (usize, usize),
    /// `a.b` of `a.b.f(...)`, `None` for `f(...)`.
    pub prefix: Option<String>,
    /// `f`.
    pub name: String,
    /// Positional arguments.
    pub args: Vec<Arg>,
    /// Keyword arguments, `None` for `**value`.
    pub keywords: Vec<(Option<String>, Arg)>,
    /// The class of the receiver of `a.f(...)` where the file shows it, as
    /// `find_helper_calls.receiver_class` reads it.
    pub receiver: Option<String>,
}

/// The calls of `source` that may be helper calls, or `None` when it is not read in Rust
/// (see [`crate::parse`]).
pub fn sites(source: &str, names: &[String]) -> Option<Vec<Site>> {
    let parsed = crate::parse::module(source)?;
    let body = &parsed.syntax().body;
    let imports = Imports::of(body);
    let pattern = mentions(names)?;
    let scan = Scan::new(body, |call: &ExprCall| match call.func.as_ref() {
        Expr::Name(_) => true,
        Expr::Attribute(attr) => dotted(&attr.value).is_some(),
        _ => false,
    });
    let ctx = Context {
        imported: &imports.imported,
        scan: &scan,
    };
    let lines = Lines::new(source);
    let mut mentions: Vec<Option<bool>> = vec![None; scan.scopes.len()];
    let mut found = Vec::new();
    for (call, stack) in &scan.calls {
        if let Some(&scope) = stack.last() {
            let wanted = *mentions[scope].get_or_insert_with(|| {
                let (start, end) = region(scan.scopes[scope], &lines, source.len());
                pattern.is_match(&source[start..end])
            });
            if !wanted {
                continue;
            }
        }
        found.push(site(call, stack, &ctx, &lines, source));
    }
    Some(found)
}

/// `\b(?:name|...)\b`, kept for the next file, which almost always asks for the same names.
pub(crate) fn mentions(names: &[String]) -> Option<Arc<Regex>> {
    static LAST: Mutex<Option<(Vec<String>, Arc<Regex>)>> = Mutex::new(None);
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((known, pattern)) = last.as_ref() {
        if known == names {
            return Some(Arc::clone(pattern));
        }
    }
    let escaped: Vec<String> = names.iter().map(|name| regex::escape(name)).collect();
    let pattern = Arc::new(Regex::new(&format!(r"\b(?:{})\b", escaped.join("|"))).ok()?);
    *last = Some((names.to_vec(), Arc::clone(&pattern)));
    Some(pattern)
}

/// The source of a scope as `ParsedSource._mentions` reads it: whole lines, from the first
/// decorator to the end of the body.
pub(crate) fn region(node: Node<'_>, lines: &Lines, len: usize) -> (usize, usize) {
    let (range, decorator): (TextRange, Option<TextRange>) = match node {
        Node::Function(function) => (
            function.range(),
            function.decorator_list.first().map(Ranged::range),
        ),
        Node::Class(class) => (
            class.range(),
            class.decorator_list.first().map(Ranged::range),
        ),
        Node::Module => return (0, len),
    };
    let start = decorator.map_or(range.start(), |d| d.start().min(range.start()));
    let first = lines.line(start.to_usize());
    let last = lines.line(range.end().to_usize().saturating_sub(1));
    (lines.start(first, len), lines.start(last + 1, len))
}

fn site(
    call: &ExprCall,
    stack: &[Scope],
    ctx: &Context<'_, '_>,
    lines: &Lines,
    source: &str,
) -> Site {
    let (prefix, name, receiver) = match call.func.as_ref() {
        Expr::Name(name) => (None, name.id.to_string(), None),
        Expr::Attribute(attr) => (
            dotted(&attr.value),
            attr.attr.to_string(),
            receiver_class(&attr.value, stack, ctx),
        ),
        _ => unreachable!("only names and dotted attributes are kept"),
    };
    let arg = |expr: &Expr| Arg {
        starred: matches!(expr, Expr::Starred(_)),
        span: (expr.start().to_usize(), expr.end().to_usize()),
        text: match expr {
            Expr::StringLiteral(text) => Some(crate::parse::text(text, source)),
            _ => None,
        },
        kind: ctx.arg_type(expr, stack),
        params: ctx.literal_params(expr, stack),
    };
    Site {
        lines: (
            lines.line(call.start().to_usize()),
            lines.line(call.end().to_usize().saturating_sub(1)),
        ),
        prefix,
        name,
        args: call.arguments.args.iter().map(arg).collect(),
        keywords: call
            .arguments
            .keywords
            .iter()
            .map(|k| (k.arg.as_ref().map(ToString::to_string), arg(&k.value)))
            .collect(),
        receiver,
    }
}

/// The class of `self`, or of a variable or `self` attribute with an evident type.
pub(crate) fn receiver_class(
    receiver: &Expr,
    stack: &[Scope],
    ctx: &Context<'_, '_>,
) -> Option<String> {
    let base = match receiver {
        Expr::Attribute(attr) => attr.value.as_ref(),
        expr => expr,
    };
    let Expr::Name(base_name) = base else {
        return None;
    };
    for pair in stack.windows(2).rev() {
        let (Node::Class(class), Node::Function(method)) =
            (ctx.scan.scopes[pair[0]], ctx.scan.scopes[pair[1]])
        else {
            continue;
        };
        let is_self = method
            .parameters
            .args
            .first()
            .is_some_and(|first| first.parameter.name.as_str() == base_name.id.as_str());
        let is_static = method
            .decorator_list
            .iter()
            .any(|d| dotted(&d.expression).as_deref() == Some("staticmethod"));
        if is_self && !is_static {
            return match receiver {
                Expr::Attribute(attr) => ctx.attribute_type(stack, attr.attr.as_str()),
                _ => Some(class.name.to_string()),
            };
        }
    }
    match receiver {
        Expr::Name(name) => ctx.type_of(stack, name.id.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str, wanted: &[&str]) -> Vec<String> {
        let wanted: Vec<String> = wanted.iter().map(ToString::to_string).collect();
        sites(source, &wanted)
            .unwrap()
            .into_iter()
            .map(|site| match site.prefix {
                Some(prefix) => format!("{prefix}.{}", site.name),
                None => site.name,
            })
            .collect()
    }

    #[test]
    fn scopes_that_never_mention_a_name_are_skipped() {
        let source = "def f():\n    g(1)\ndef h():\n    k(2)\nk(3)\n";
        assert_eq!(names(source, &["g"]), ["g", "k"]);
    }

    #[test]
    fn decorators_belong_to_their_function() {
        let source = "@fetch\ndef f():\n    g(1)\n";
        assert_eq!(names(source, &["fetch"]), ["g"]);
    }

    #[test]
    fn receivers() {
        let source = "class Db:\n    def run(self):\n        self.q.fetch(1)\n        self.fetch(2)\n    def __init__(self):\n        self.q = Repo()\ndb: Repo = Repo()\ndb.fetch(3)\n";
        let found = sites(source, &["fetch".to_string()]).unwrap();
        let receivers: Vec<_> = found.iter().map(|s| s.receiver.as_deref()).collect();
        assert_eq!(receivers, [Some("Repo"), Some("Db"), None, Some("Repo")]);
    }
}
