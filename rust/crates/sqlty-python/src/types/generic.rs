//! Generic row types with their type arguments applied: `SQL[Row[int]]` for `class Row[T]`.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use ruff_python_ast::visitor::transformer::{walk_expr, Transformer};
use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextRange, TextSize};

use super::info::{Annotation, TypeInfo};
use super::pydantic::Known;
use super::symbols::ModuleTypes;
use crate::unparse::unparse;

/// Whether `text` opens with a character Python's `\w` matches.
fn opens_word(text: &str) -> bool {
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]").expect("a valid pattern"));
    WORD.is_match(text)
}

/// `re.sub(r"\b(params)\b", ...)`: `text` with each whole word that is one of `params`
/// replaced by what `with` gives for it.
fn replace_words(text: &str, params: &[String], with: impl Fn(&str) -> String) -> String {
    let boundary = |at: usize| {
        let before = text[..at]
            .char_indices()
            .next_back()
            .is_some_and(|(start, _)| opens_word(&text[start..]));
        before != opens_word(&text[at..])
    };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(c) = text[at..].chars().next() {
        let found = params.iter().find(|param| {
            text[at..].starts_with(param.as_str()) && boundary(at) && boundary(at + param.len())
        });
        match found {
            Some(param) => {
                out.push_str(&with(param));
                at += param.len();
            }
            None => {
                out.push(c);
                at += c.len_utf8();
            }
        }
    }
    out
}

/// `_Substitute`: swaps each name of `names` for its expression, wherever it stands.
struct Substitute<'a>(&'a [(&'a str, Expr)]);

impl Transformer for Substitute<'_> {
    fn visit_expr(&self, expr: &mut Expr) {
        if let Expr::Name(name) = expr {
            let found = self
                .0
                .iter()
                .rev()
                .find(|(param, _)| *param == name.id.as_str());
            if let Some((_, with)) = found {
                *expr = with.clone();
            }
            return;
        }
        walk_expr(self, expr);
    }
}

/// The expression written in `range` of `text`, read as if in parentheses.
fn parse_at(text: &str, range: TextRange) -> Option<Expr> {
    let stack = (1 << 20) + range.len().to_usize() * (16 << 10);
    let parsed = stacker::maybe_grow(stack, stack, || {
        ruff_python_parser::parse_parenthesized_expression_range(text, range)
    });
    Some(*parsed.ok()?.into_syntax().body)
}

/// `specialize`: `info` with the type parameters of its class replaced by `args`,
/// expressions of `source`.
///
/// `None` when the class takes a different number of arguments. An argument that cannot be
/// resolved leaves the fields it fills unknown. `names` are those of the module that names
/// `Row[...]`.
pub fn specialize(
    info: &TypeInfo,
    args: &[Expr],
    source: &str,
    names: &ModuleTypes,
) -> Option<TypeInfo> {
    let params = &info.type_params;
    if params.is_empty() || params.len() != args.len() {
        return None;
    }
    let texts: Vec<String> = args.iter().map(|arg| unparse(arg, source)).collect();
    let text_of = |word: &str| {
        params
            .iter()
            .rposition(|param| param == word)
            .map_or(word, |index| texts[index].as_str())
            .to_string()
    };

    let mut out = info.clone();
    out.fields = info
        .fields
        .iter()
        .map(|(name, annotation)| (name.clone(), replace_words(annotation, params, text_of)))
        .collect();
    // The arguments as written, each as the subscript of `x[...]`, which also reads `*Ts`
    // and `1:2`; an annotation is put before them, so its nodes and theirs share a text.
    let mut tail = String::new();
    let mut ranges = Vec::new();
    for arg in args {
        tail.push_str("\nx[");
        let start = tail.len();
        tail.push_str(source.get(arg.range().to_std_range())?);
        ranges.push((start - 2, start, tail.len()));
        tail.push(']');
    }
    let size = |len: usize| TextSize::try_from(len).ok();
    out.annotations = info
        .annotations
        .iter()
        .filter_map(|(name, annotation)| {
            let written = annotation
                .source
                .get(annotation.expr.range().to_std_range())?;
            let text = format!("{written}{tail}");
            let mut expr = parse_at(&text, TextRange::new(size(0)?, size(written.len())?))?;
            let mut nodes = Vec::new();
            for ((param, arg), (open, start, end)) in params.iter().zip(args).zip(&ranges) {
                let at = written.len();
                let range = TextRange::new(size(at + open)?, size(at + end + 1)?);
                let Expr::Subscript(subscript) = parse_at(&text, range)? else {
                    return None;
                };
                let node = match *subscript.slice {
                    Expr::Tuple(tuple)
                        if tuple.range().start().to_usize() == at + start
                            && !arg.is_tuple_expr() =>
                    {
                        tuple.elts.into_iter().next()?
                    }
                    slice => slice,
                };
                nodes.push((param.as_str(), node));
            }
            Substitute(&nodes).visit_expr(&mut expr);
            let annotation = Annotation {
                expr,
                source: Arc::from(text),
            };
            Some((name, annotation))
        })
        .collect();
    let resolver = names.source.resolver(Known::Imported);
    out.field_types = info
        .field_types
        .iter()
        .map(|(name, tp)| {
            let tp = match out.annotations.get(name) {
                Some(annotation) => resolver.resolve(&annotation.expr, &annotation.source, names),
                None => tp.clone(),
            };
            (name, tp)
        })
        .collect();
    Some(out)
}
