//! What finding helper calls reads of a module: which scopes mention a helper name, and the
//! class of a call's receiver.

use std::sync::{Arc, Mutex};

use regex::Regex;
use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextRange};

use crate::scopes::{dotted, Context, Lines, Node, Scope};

/// `\b(?:name|...)\b`, kept for the next file, which almost always asks for the same names.
pub(crate) fn mentions(names: &[String]) -> Option<Arc<Regex>> {
    type Last = Option<(Vec<String>, Option<Arc<Regex>>)>;
    static LAST: Mutex<Last> = Mutex::new(None);
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((known, pattern)) = last.as_ref() {
        if known == names {
            return pattern.clone();
        }
    }
    let escaped: Vec<String> = names.iter().map(|name| regex::escape(name)).collect();
    // Too many names for one pattern: kept too, so the next file does not try again.
    let pattern = Regex::new(&format!(r"\b(?:{})\b", escaped.join("|")))
        .ok()
        .map(Arc::new);
    *last = Some((names.to_vec(), pattern.clone()));
    pattern
}

/// The source of a scope as a scan for helper names reads it: whole lines, from the first
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

/// The class of `self`, or of a variable or `self` attribute with an evident type.
pub(crate) fn receiver_class(
    receiver: &Expr,
    stack: &[Scope],
    ctx: &Context<'_, '_, '_>,
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn the_module_region_is_the_whole_source() {
        let source = "x = 1\ny = 2\n";
        assert_eq!(
            region(Node::Module, &Lines::new(source), source.len()),
            (0, source.len())
        );
    }
}
