//! The t-strings `sqlty._drivers.find_template_calls` reads: the query psycopg sends for each,
//! a field that passes a value becoming `%s`, and the types of those values.

use ruff_python_ast::{
    ConversionFlag, Expr, ExprTString, InterpolatedStringElement, InterpolatedStringFormatSpec,
};
use ruff_python_parser::parse_expression;
use ruff_text_size::Ranged;

use crate::module::Imports;
use crate::scopes::{Context, Lines, Scan, Scope};

/// A t-string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// The line it starts on.
    pub line: usize,
    /// Byte offsets.
    pub span: (usize, usize),
    /// The query, `%` doubled and each field `%s`; `None` when a field does not pass a value
    /// (a conversion, or a format spec other than `s`, `b` or `t`).
    pub query: Option<String>,
    /// The type of the value of each field, as `sqlty._params.arg_type` reads it.
    pub params: Vec<Option<String>>,
}

/// Every t-string of `source`, in source order, or `None` when it is not read in Rust
/// (see [`crate::parse`]).
pub fn templates(source: &str) -> Option<Vec<Template>> {
    let parsed = crate::parse::module(source)?;
    let body = &parsed.syntax().body;
    let imports = Imports::of(body);
    let scan = Scan::new(body, |_| false);
    let ctx = Context {
        imported: &imports.imported,
        scan: &scan,
    };
    let lines = Lines::new(source);
    let mut found: Vec<Template> = scan
        .templates
        .iter()
        .map(|(template, stack)| read(template, stack, &ctx, &lines, source))
        .collect();
    found.sort_by_key(|template| template.span);
    Some(found)
}

fn read(
    template: &ExprTString,
    stack: &[Scope],
    ctx: &Context<'_, '_>,
    lines: &Lines,
    source: &str,
) -> Template {
    let span = (template.start().to_usize(), template.end().to_usize());
    let written = &source[template.range()];
    // `ast` reads a newline in the source as `\n`, so read the t-string again with those.
    let reparsed = written
        .contains('\r')
        .then(|| {
            let normalized = written.replace("\r\n", "\n").replace('\r', "\n");
            parse_expression(&format!("({normalized})")).ok()
        })
        .flatten();
    let template = match reparsed
        .as_ref()
        .map(|parsed| parsed.syntax().body.as_ref())
    {
        Some(Expr::TString(inner)) => inner,
        _ => template,
    };
    let mut query = String::new();
    let mut params = Vec::new();
    let mut complete = true;
    'parts: for part in template.value.as_slice() {
        for element in &part.elements {
            match element {
                InterpolatedStringElement::Literal(literal) => {
                    query.push_str(&literal.value.replace('%', "%%"));
                }
                InterpolatedStringElement::Interpolation(field) => {
                    let mut conversion = field.conversion;
                    if let Some(debug) = &field.debug_text {
                        query.push_str(&debug.as_str().replace('%', "%%"));
                        if conversion == ConversionFlag::None && field.format_spec.is_none() {
                            conversion = ConversionFlag::Repr;
                        }
                    }
                    if conversion != ConversionFlag::None || !wire(field.format_spec.as_deref()) {
                        complete = false;
                        break 'parts;
                    }
                    query.push_str("%s");
                    params.push(ctx.arg_type(&field.expression, stack));
                }
            }
        }
    }
    Template {
        line: lines.line(span.0),
        span,
        query: complete.then_some(query),
        params,
    }
}

/// Whether a field with `spec` passes its value: no spec, or `s`, `b` or `t` as plain text.
fn wire(spec: Option<&InterpolatedStringFormatSpec>) -> bool {
    let Some(spec) = spec else {
        return true;
    };
    let mut text = String::new();
    for element in &spec.elements {
        match element {
            InterpolatedStringElement::Literal(literal) => text.push_str(&literal.value),
            InterpolatedStringElement::Interpolation(_) => return false,
        }
    }
    matches!(text.as_str(), "s" | "b" | "t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_and_types() {
        let source = "def f(n: int):\n    q = t\"--sql\\nSELECT {n}, {x:b}, 5%\" t'{n=:s}'\n\
                      r = t\"{n!r}\"\nw = t\"{n:{w}}\"\nv = t\"{n:}\"\n";
        let found = templates(source).unwrap();
        let queries: Vec<_> = found.iter().map(|t| t.query.as_deref()).collect();
        assert_eq!(
            queries,
            [Some("--sql\nSELECT %s, %s, 5%%n=%s"), None, None, None]
        );
        assert_eq!(
            found[0].params,
            [Some("int".to_string()), None, Some("int".to_string())]
        );
        assert_eq!((found[0].line, found[1].line), (2, 3));
    }

    #[test]
    fn newlines_read_as_ast_reads_them() {
        let found = templates("q = t\"\"\"--sql\r\nSELECT {1}\r\n\"\"\"\r\n").unwrap();
        assert_eq!(found[0].query.as_deref(), Some("--sql\nSELECT %s\n"));
        assert_eq!(found[0].span, (4, 30));
    }
}
