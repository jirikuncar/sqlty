//! Generic row types with their type arguments applied: `SQL[Row[int]]` for `class Row[T]`.

use std::sync::Arc;

use regex::Regex;
use ruff_python_ast::Expr;

use super::info::{Annotation, TypeInfo};
use super::pydantic::Known;
use super::symbols::ModuleTypes;
use crate::unparse::{unparse, unparse_with, Substitutions};

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
    let pattern: Vec<String> = params.iter().map(|param| regex::escape(param)).collect();
    let words = Regex::new(&format!(r"\b({})\b", pattern.join("|"))).ok()?;
    let nodes: Substitutions<'_> = params
        .iter()
        .map(String::as_str)
        .zip(args.iter().map(|arg| (arg, source)))
        .collect();
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
        .map(|(name, annotation)| {
            let replaced =
                words.replace_all(annotation, |found: &regex::Captures<'_>| text_of(&found[0]));
            (name.clone(), replaced.into_owned())
        })
        .collect();
    out.annotations = info
        .annotations
        .iter()
        .filter_map(|(name, annotation)| {
            let text = unparse_with(&annotation.expr, &annotation.source, &nodes);
            let parsed = crate::parse::expression(&text)?;
            let annotation = Annotation {
                expr: *parsed.into_syntax().body,
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
